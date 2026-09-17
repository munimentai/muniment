// Package intercom reads an Intercom workspace behind the reader contract:
// three objects, contacts, companies and conversations, each flattened to
// text fields a mapping can point at. It uses the REST API over net/http
// alone, with an access token as the bearer, and writes nothing back.
// Backfill walks cursor pagination, and Delta is one search for the newest
// change after the cursor's mark where the API searches, and a first-page
// read where it does not.
package intercom

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Intercom's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.intercom.io"

// APIVersion is the Intercom-Version header every call sends.
const APIVersion = "2.11"

// PageLimit is the most records one page returns for contacts and
// conversations. Companies list fifty.
const PageLimit = 150

const companyPageLimit = 50

const sampleRows = 100

type (
	Failure          = contract.Failure
	Cursor           = contract.Cursor
	ObjectInfo       = contract.ObjectInfo
	FieldDescription = contract.FieldDescription
	Description      = contract.Description
	Row              = contract.Row
	Page             = contract.Page
	Delta            = contract.Delta
)

// Source is one Intercom workspace.
type Source struct {
	token   string
	baseURL string
	client  *http.Client
}

// New opens a source on an access token. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{token: contract.Credentials(secret)["token"], baseURL: strings.TrimRight(baseURL, "/"), client: &http.Client{Timeout: 30 * time.Second}}
}

type record = map[string]any

type object struct {
	name   string
	label  string
	path   string
	key    string
	limit  int
	search bool
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "contacts", label: "Contacts", path: "/contacts", key: "data", limit: PageLimit, search: true,
		fields: []field{
			{"id", "id", text("id")},
			{"external_id", "string", text("external_id")},
			{"role", "string", text("role")},
			{"name", "string", text("name")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", text("phone")},
			{"company", "id", firstListed("companies", "data")},
			{"companies", "string", allListed("companies", "data")},
			{"owner", "string", text("owner_id")},
			{"city", "string", nested("location", "city")},
			{"country", "string", nested("location", "country")},
			{"unsubscribed", "boolean", boolean("unsubscribed_from_emails")},
			{"signed_up", "date-time", unix("signed_up_at")},
			{"last_seen", "date-time", unix("last_seen_at")},
			{"created", "date-time", unix("created_at")},
			{"modified", "date-time", unix("updated_at")},
		},
	},
	{
		name: "companies", label: "Companies", path: "/companies", key: "data", limit: companyPageLimit, search: false,
		fields: []field{
			{"id", "id", text("id")},
			{"company_id", "string", text("company_id")},
			{"name", "string", text("name")},
			{"website", "string", text("website")},
			{"domain", "domain", websiteDomain},
			{"industry", "string", text("industry")},
			{"size", "number", text("size")},
			{"plan", "string", nested("plan", "name")},
			{"monthly_spend", "number", text("monthly_spend")},
			{"sessions", "number", text("session_count")},
			{"users", "number", text("user_count")},
			{"remote_created", "date-time", unix("remote_created_at")},
			{"created", "date-time", unix("created_at")},
			{"modified", "date-time", unix("updated_at")},
		},
	},
	{
		name: "conversations", label: "Conversations", path: "/conversations", key: "conversations", limit: PageLimit, search: true,
		fields: []field{
			{"id", "id", text("id")},
			{"title", "string", text("title")},
			{"subject", "string", nested("source", "subject")},
			{"channel", "string", nested("source", "type")},
			{"state", "string", text("state")},
			{"status", "string", conversationStatus},
			{"priority", "string", conversationPriority},
			{"read", "boolean", boolean("read")},
			{"contact", "id", firstListed("contacts", "contacts")},
			{"contacts", "string", allListed("contacts", "contacts")},
			{"assignee", "string", text("admin_assignee_id")},
			{"team", "string", text("team_assignee_id")},
			{"tags", "string", tagNames},
			{"waiting_since", "date-time", unix("waiting_since")},
			{"snoozed_until", "date-time", unix("snoozed_until")},
			{"created", "date-time", unix("created_at")},
			{"modified", "date-time", unix("updated_at")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Intercom has no %s to read. The objects are contacts, companies and conversations.", name)}
}

// Objects proves the token with one small call and lists the three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.token == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Intercom with its access token first."}
	}
	if _, _, err := s.list(objects[0], url.Values{"per_page": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Custom attributes become columns from the records read, because the
// workspace names them in the record itself.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	limit := sampleRows
	if limit > object.limit {
		limit = object.limit
	}
	items, _, err := s.list(*object, url.Values{"per_page": {strconv.Itoa(limit)}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	columns := make([]contract.Column, 0, len(object.fields))
	for _, f := range object.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	columns = append(columns, customColumns(items, columns)...)
	return &Description{Source: "intercom", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: len(rows), Hash: newestUpdated(items, ""), Counted: false}, nil
}

// Page reads one page after the cursor's token.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > object.limit {
		limit = object.limit
	}
	offset, hash, after := 0, "", ""
	if cursor != nil {
		offset, hash, after = cursor.Offset, cursor.Hash, cursor.Token
	}
	query := url.Values{"per_page": {strconv.Itoa(limit)}}
	if after != "" {
		if strings.HasPrefix(after, "page:") {
			query.Set("page", strings.TrimPrefix(after, "page:"))
		} else {
			query.Set("starting_after", after)
		}
	}
	items, next, err := s.list(*object, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta searches for one record changed after the mark where the API
// searches, and reads the first page where it does not.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	var items []record
	if object.search {
		body := map[string]any{
			"pagination": map[string]any{"per_page": 1},
			"sort":       map[string]any{"field": "updated_at", "order": "descending"},
		}
		if mark != "" {
			body["query"] = map[string]any{"field": "updated_at", "operator": ">", "value": markNumber(mark)}
		} else {
			body["query"] = map[string]any{"field": "updated_at", "operator": ">", "value": 0}
		}
		items, err = s.search(object, body)
	} else {
		items, _, err = s.list(*object, url.Values{"per_page": {strconv.Itoa(object.limit)}, "order": {"desc"}})
	}
	if err != nil {
		return nil, err
	}
	if len(items) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestUpdated(items, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

func markNumber(mark string) any {
	if number, err := strconv.ParseInt(mark, 10, 64); err == nil {
		return number
	}
	return mark
}

func (s *Source) list(object object, query url.Values) ([]record, string, error) {
	body, status, err := s.do(http.MethodGet, object.path, query, nil)
	if err != nil {
		return nil, "", err
	}
	if err := refusal(object.name, status, body); err != nil {
		return nil, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Intercom's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := listed[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Intercom's %s do not parse: %s.", object.key, err)}
		}
	}
	var pages struct {
		Next json.RawMessage `json:"next"`
	}
	if raw, ok := listed["pages"]; ok {
		_ = json.Unmarshal(raw, &pages)
	}
	return items, nextToken(pages.Next), nil
}

// nextToken reads the page pointer Intercom answers: a cursor object with
// `starting_after`, a page object with `page`, or a URL naming either.
func nextToken(raw json.RawMessage) string {
	if len(raw) == 0 || string(raw) == "null" {
		return ""
	}
	var pointer struct {
		StartingAfter string `json:"starting_after"`
		Page          any    `json:"page"`
	}
	if json.Unmarshal(raw, &pointer) == nil {
		if pointer.StartingAfter != "" {
			return pointer.StartingAfter
		}
		if page := contract.Scalar(pointer.Page); page != "" {
			return "page:" + page
		}
	}
	var link string
	if json.Unmarshal(raw, &link) == nil && link != "" {
		if parsed, err := url.Parse(link); err == nil {
			if after := parsed.Query().Get("starting_after"); after != "" {
				return after
			}
			if page := parsed.Query().Get("page"); page != "" {
				return "page:" + page
			}
		}
	}
	return ""
}

func (s *Source) search(object *object, body map[string]any) ([]record, error) {
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, &Failure{Code: "source", Message: err.Error()}
	}
	answer, status, err := s.do(http.MethodPost, object.path+"/search", nil, encoded)
	if err != nil {
		return nil, err
	}
	if err := refusal(object.name, status, answer); err != nil {
		return nil, err
	}
	var found map[string]json.RawMessage
	if err := json.Unmarshal(answer, &found); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Intercom's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := found[object.key]; ok {
		_ = json.Unmarshal(raw, &items)
	}
	return items, nil
}

func (s *Source) do(method, path string, query url.Values, body []byte) ([]byte, int, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}
	request, err := http.NewRequest(method, target, reader)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Intercom-Version", APIVersion)
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Intercom did not answer: %s.", err)}
	}
	defer response.Body.Close()
	answer, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Intercom's answer did not read: %s.", err)}
	}
	return answer, response.StatusCode, nil
}

func refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Intercom refused the access token. Connect Intercom again with a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Intercom refused the request for %s: %s. The app needs read access to them.", object, intercomMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Intercom asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Intercom answered %d: %s", status, intercomMessage(body))}
	}
	return nil
}

func intercomMessage(body []byte) string {
	var failure struct {
		Errors []struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Errors) > 0 {
		if failure.Errors[0].Message != "" {
			return failure.Errors[0].Message
		}
		return failure.Errors[0].Code
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// customColumns names every custom attribute the records carry, sorted,
// skipping one that would shadow a core column.
func customColumns(items []record, core []contract.Column) []contract.Column {
	taken := map[string]bool{}
	for _, column := range core {
		taken[column.Name] = true
	}
	seen := map[string]bool{}
	names := []string{}
	for _, item := range items {
		attributes, _ := item["custom_attributes"].(map[string]any)
		for key := range attributes {
			name := columnName(key)
			if name == "" || taken[name] || seen[name] {
				continue
			}
			seen[name] = true
			names = append(names, name)
		}
	}
	sort.Strings(names)
	columns := make([]contract.Column, 0, len(names))
	for _, name := range names {
		columns = append(columns, contract.Column{Name: name, Guess: "string"})
	}
	return columns
}

// row flattens one record: the core fields, then every custom attribute as
// its own column named as the workspace named it.
func (o *object) row(item record) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	attributes, _ := item["custom_attributes"].(map[string]any)
	keys := make([]string, 0, len(attributes))
	for key := range attributes {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		name := columnName(key)
		if _, core := row[name]; core || name == "" {
			continue
		}
		row[name] = contract.Scalar(attributes[key])
	}
	return row
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

func nested(parent, key string) func(record) string {
	return func(item record) string {
		if child, ok := item[parent].(map[string]any); ok {
			return contract.Scalar(child[key])
		}
		return ""
	}
}

func boolean(key string) func(record) string {
	return func(item record) string {
		switch value := item[key].(type) {
		case bool:
			if value {
				return "yes"
			}
			return "no"
		default:
			return ""
		}
	}
}

// unix reads a second count as RFC 3339 in UTC.
func unix(key string) func(record) string {
	return func(item record) string {
		seconds, ok := item[key].(float64)
		if !ok || seconds <= 0 {
			return ""
		}
		return time.Unix(int64(seconds), 0).UTC().Format(time.RFC3339)
	}
}

func listedIDs(item record, parent, key string) []string {
	group, ok := item[parent].(map[string]any)
	if !ok {
		return nil
	}
	list, ok := group[key].([]any)
	if !ok {
		return nil
	}
	ids := []string{}
	for _, entry := range list {
		child, _ := entry.(map[string]any)
		if id := contract.Scalar(child["id"]); id != "" {
			ids = append(ids, id)
		}
	}
	return ids
}

func firstListed(parent, key string) func(record) string {
	return func(item record) string {
		ids := listedIDs(item, parent, key)
		if len(ids) == 0 {
			return ""
		}
		return ids[0]
	}
}

func allListed(parent, key string) func(record) string {
	return func(item record) string { return strings.Join(listedIDs(item, parent, key), ",") }
}

func tagNames(item record) string {
	group, ok := item["tags"].(map[string]any)
	if !ok {
		return ""
	}
	list, ok := group["tags"].([]any)
	if !ok {
		return ""
	}
	names := []string{}
	for _, entry := range list {
		tag, _ := entry.(map[string]any)
		if name := contract.Scalar(tag["name"]); name != "" {
			names = append(names, name)
		}
	}
	sort.Strings(names)
	return strings.Join(names, ",")
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

func websiteDomain(item record) string {
	site := strings.TrimSpace(contract.Scalar(item["website"]))
	if site == "" {
		return ""
	}
	if !strings.Contains(site, "://") {
		site = "https://" + site
	}
	parsed, err := url.Parse(site)
	if err != nil || parsed.Host == "" {
		return ""
	}
	return strings.TrimPrefix(strings.ToLower(parsed.Hostname()), "www.")
}

// conversationStatus folds Intercom's states onto the ticket kind's: open
// is open, snoozed is pending, closed is closed.
func conversationStatus(item record) string {
	switch contract.Scalar(item["state"]) {
	case "open":
		return "open"
	case "snoozed":
		return "pending"
	case "closed":
		return "closed"
	default:
		return ""
	}
}

func conversationPriority(item record) string {
	if contract.Scalar(item["priority"]) == "priority" {
		return "high"
	}
	return ""
}

// newestUpdated is the change mark: the latest updated_at second count in
// a list as text, or the previous mark.
func newestUpdated(items []record, previous string) string {
	var newest int64
	for _, item := range items {
		if seconds, ok := item["updated_at"].(float64); ok && int64(seconds) > newest {
			newest = int64(seconds)
		}
	}
	if newest == 0 {
		return previous
	}
	if before, err := strconv.ParseInt(previous, 10, 64); err == nil && before > newest {
		return previous
	}
	return strconv.FormatInt(newest, 10)
}
