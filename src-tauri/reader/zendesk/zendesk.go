// Package zendesk reads a Zendesk account behind the reader contract: three
// objects, tickets, users and organizations, each flattened to text fields
// a mapping can point at. It uses the v2 REST API over net/http alone, with
// the agent's email and API token as basic auth, and writes nothing back.
// Backfill walks cursor pagination, and Delta is one search for the newest
// change after the cursor's mark.
package zendesk

import (
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

// PageLimit is the most records one cursor page returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
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

// Source is one Zendesk account.
type Source struct {
	baseURL   string
	email     string
	token     string
	client    *http.Client
	custom    map[string][]customField
	subdomain string
	lastRetry string
}

// customField is one custom field as a column: the ticket field id or the
// user and organization field key, and the column name.
type customField struct {
	id     string
	key    string
	column contract.Column
}

// New opens a source on the credentials the panel packs: the subdomain,
// the agent's email and the API token. A base URL points a test at a fake.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	subdomain := strings.TrimSpace(credentials["subdomain"])
	subdomain = strings.TrimSuffix(strings.TrimPrefix(strings.TrimPrefix(subdomain, "https://"), "http://"), ".zendesk.com")
	if baseURL == "" && subdomain != "" {
		baseURL = "https://" + subdomain + ".zendesk.com"
	}
	return &Source{
		baseURL:   strings.TrimRight(baseURL, "/"),
		email:     credentials["email"],
		token:     credentials["api_token"],
		client:    &http.Client{Timeout: 30 * time.Second},
		custom:    map[string][]customField{},
		subdomain: subdomain,
	}
}

type record = map[string]any

type object struct {
	name   string
	label  string
	path   string
	key    string
	search string
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "tickets", label: "Tickets", path: "/api/v2/tickets", key: "tickets", search: "ticket",
		fields: []field{
			{"id", "id", text("id")},
			{"subject", "string", text("subject")},
			{"description", "string", text("description")},
			{"status_name", "string", text("status")},
			{"status", "string", ticketStatus},
			{"priority", "string", ticketPriority},
			{"type", "string", text("type")},
			{"requester", "id", text("requester_id")},
			{"assignee", "string", text("assignee_id")},
			{"organization", "id", text("organization_id")},
			{"group", "string", text("group_id")},
			{"channel", "string", nested("via", "channel")},
			{"tags", "string", tags("tags")},
			{"due_at", "date-time", when("due_at")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "users", label: "Users", path: "/api/v2/users", key: "users", search: "user",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", text("phone")},
			{"role", "string", text("role")},
			{"organization", "id", text("organization_id")},
			{"active", "boolean", boolean("active")},
			{"verified", "boolean", boolean("verified")},
			{"time_zone", "string", text("time_zone")},
			{"locale", "string", text("locale")},
			{"tags", "string", tags("tags")},
			{"last_login", "date-time", when("last_login_at")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "organizations", label: "Organizations", path: "/api/v2/organizations", key: "organizations", search: "organization",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"domain", "domain", firstOf("domain_names")},
			{"domains", "string", tags("domain_names")},
			{"details", "string", text("details")},
			{"notes", "string", text("notes")},
			{"group", "string", text("group_id")},
			{"shared_tickets", "boolean", boolean("shared_tickets")},
			{"tags", "string", tags("tags")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Zendesk has no %s to read. The objects are tickets, users and organizations.", name)}
}

// Objects proves the credentials with one small call and lists the three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.baseURL == "" || s.email == "" || s.token == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Zendesk with its subdomain, agent email and API token first."}
	}
	if _, _, err := s.list(objects[1], url.Values{"page[size]": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	items, _, err := s.list(*object, url.Values{"page[size]": {strconv.Itoa(sampleRows)}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	columns := make([]contract.Column, 0, len(object.fields)+len(custom))
	for _, f := range object.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	for _, extra := range custom {
		columns = append(columns, extra.column)
	}
	return &Description{Source: "zendesk", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: len(rows), Hash: newestUpdated(items, ""), Counted: false}, nil
}

// Page reads one cursor page after the cursor's token.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, after := 0, "", ""
	if cursor != nil {
		offset, hash, after = cursor.Offset, cursor.Hash, cursor.Token
	}
	query := url.Values{"page[size]": {strconv.Itoa(limit)}}
	if after != "" {
		query.Set("page[after]", after)
	}
	custom := s.customFields(object)
	items, next, err := s.list(*object, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	hash = newestUpdated(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks the search endpoint for the newest record changed after the
// mark, one result, so a cycle spends one search per object.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	term := "type:" + object.search
	if mark != "" {
		term += " updated>" + mark
	}
	body, status, err := s.get("/api/v2/search", url.Values{"query": {term}, "sort_by": {"updated_at"}, "sort_order": {"desc"}, "per_page": {"1"}})
	if err != nil {
		return nil, err
	}
	if err := s.refusal(object.name, status, body, nil); err != nil {
		return nil, err
	}
	var found struct {
		Results []record `json:"results"`
	}
	if err := json.Unmarshal(body, &found); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Zendesk's answer does not parse: %s.", err)}
	}
	if len(found.Results) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestUpdated(found.Results, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// customFields reads the account's own fields on an object once per
// source: ticket fields by id, user and organization fields by key.
func (s *Source) customFields(object *object) []customField {
	if cached, ok := s.custom[object.name]; ok {
		return cached
	}
	taken := map[string]bool{}
	for _, f := range object.fields {
		taken[f.name] = true
	}
	listed := []customField{}
	path := map[string]string{"tickets": "/api/v2/ticket_fields", "users": "/api/v2/user_fields", "organizations": "/api/v2/organization_fields"}[object.name]
	body, status, err := s.get(path, url.Values{})
	if err == nil && status == http.StatusOK {
		var answer map[string][]struct {
			ID        any    `json:"id"`
			Key       string `json:"key"`
			Title     string `json:"title"`
			Type      string `json:"type"`
			Removable bool   `json:"removable"`
			Active    bool   `json:"active"`
		}
		if json.Unmarshal(body, &answer) == nil {
			for _, fields := range answer {
				for _, f := range fields {
					if !f.Active || (object.name == "tickets" && !f.Removable) {
						continue
					}
					name := f.Key
					if object.name == "tickets" {
						name = columnName(f.Title)
					}
					if name == "" || taken[name] {
						continue
					}
					taken[name] = true
					listed = append(listed, customField{id: contract.Scalar(f.ID), key: f.Key, column: contract.Column{Name: name, Guess: guessType(f.Type)}})
				}
			}
		}
	}
	sort.Slice(listed, func(a, b int) bool { return listed[a].column.Name < listed[b].column.Name })
	s.custom[object.name] = listed
	return listed
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

func columnName(title string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(title), "_"), "_")
}

func guessType(fieldType string) string {
	switch fieldType {
	case "integer", "decimal":
		return "number"
	case "checkbox":
		return "boolean"
	case "date":
		return "date"
	default:
		return "string"
	}
}

// list fetches one cursor page and answers its records and the next cursor.
func (s *Source) list(object object, query url.Values) ([]record, string, error) {
	body, status, err := s.get(object.path, query)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(object.name, status, body, nil); err != nil {
		return nil, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Zendesk's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := listed[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Zendesk's %s do not parse: %s.", object.key, err)}
		}
	}
	var meta struct {
		HasMore     bool   `json:"has_more"`
		AfterCursor string `json:"after_cursor"`
	}
	if raw, ok := listed["meta"]; ok {
		_ = json.Unmarshal(raw, &meta)
	}
	next := ""
	if meta.HasMore {
		next = meta.AfterCursor
	}
	return items, next, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.SetBasicAuth(s.email+"/token", s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Zendesk did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Zendesk's answer did not read: %s.", err)}
	}
	s.lastRetry = response.Header.Get("Retry-After")
	return body, response.StatusCode, nil
}

func (s *Source) refusal(object string, status int, body []byte, _ http.Header) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Zendesk refused the email and API token. Connect Zendesk again with an agent's email and a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Zendesk refused the request for %s: %s. The agent needs access to them.", object, zendeskMessage(body))}
	case status == http.StatusTooManyRequests:
		if seconds, err := strconv.Atoi(strings.TrimSpace(s.lastRetry)); err == nil && seconds > 0 {
			return &Failure{Code: "rate_limited", Message: fmt.Sprintf("Zendesk asked the reader to wait %d seconds. Run again then.", seconds)}
		}
		return &Failure{Code: "rate_limited", Message: "Zendesk asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Zendesk answered %d: %s", status, zendeskMessage(body))}
	}
	return nil
}

func zendeskMessage(body []byte) string {
	var failure struct {
		Error       any    `json:"error"`
		Description string `json:"description"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if failure.Description != "" {
			return failure.Description
		}
		if text := contract.Scalar(failure.Error); text != "" {
			return text
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record: the core fields, then every custom field as its
// own column. A ticket's custom fields are a list of id and value; a user's
// or an organization's are a map by key.
func (o *object) row(item record, custom []customField) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	if len(custom) == 0 {
		return row
	}
	values := map[string]string{}
	if list, ok := item["custom_fields"].([]any); ok {
		for _, entry := range list {
			pair, _ := entry.(map[string]any)
			values[contract.Scalar(pair["id"])] = customValue(pair["value"])
		}
	}
	fieldsKey := map[string]string{"users": "user_fields", "organizations": "organization_fields"}[o.name]
	keyed, _ := item[fieldsKey].(map[string]any)
	for _, extra := range custom {
		if o.name == "tickets" {
			row[extra.column.Name] = values[extra.id]
		} else {
			row[extra.column.Name] = customValue(keyed[extra.key])
		}
	}
	return row
}

func customValue(value any) string {
	if list, ok := value.([]any); ok {
		return contract.Joined(list, true)
	}
	return contract.Scalar(value)
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

func tags(key string) func(record) string {
	return func(item record) string { return contract.Joined(item[key], true) }
}

func firstOf(key string) func(record) string {
	return func(item record) string {
		list, ok := item[key].([]any)
		if !ok || len(list) == 0 {
			return ""
		}
		return strings.ToLower(contract.Scalar(list[0]))
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	return value
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// ticketStatus folds Zendesk's statuses onto the ticket kind's: new and
// open are open, pending and hold are pending, solved is resolved.
func ticketStatus(item record) string {
	switch contract.Scalar(item["status"]) {
	case "new", "open":
		return "open"
	case "pending", "hold":
		return "pending"
	case "solved":
		return "resolved"
	case "closed":
		return "closed"
	default:
		return ""
	}
}

// ticketPriority folds normal onto medium and keeps the rest.
func ticketPriority(item record) string {
	switch priority := contract.Scalar(item["priority"]); priority {
	case "normal":
		return "medium"
	case "low", "high", "urgent":
		return priority
	default:
		return ""
	}
}

// newestUpdated is the change mark: the latest updated_at in a list as
// Zendesk writes it, so a search reads it back, or the previous mark.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["updated_at"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(time.RFC3339Nano, previous); err == nil && before.After(newest) {
		return previous
	}
	return newest.UTC().Format(time.RFC3339)
}
