// Package freshdesk reads a Freshdesk account behind the reader contract:
// three objects, tickets, contacts and companies, each flattened to text
// fields a mapping can point at. It uses the v2 REST API over net/http
// alone, with the API key as basic auth, and writes nothing back. Backfill
// walks page numbers through the Link header, and Delta reads the records
// updated since the cursor's mark.
package freshdesk

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

// PageLimit is the most records one page returns.
const PageLimit = 100

const sampleRows = 100

// epoch is the updated_since a first ticket read passes, because the list
// answers the last thirty days alone without one.
const epoch = "1970-01-01T00:00:00Z"

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

// Source is one Freshdesk account.
type Source struct {
	baseURL   string
	key       string
	client    *http.Client
	lastRetry string
}

// New opens a source on the credentials the panel packs: the domain and
// the API key. A base URL points a test at a fake.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	domain := strings.TrimSpace(credentials["domain"])
	domain = strings.TrimSuffix(strings.TrimPrefix(strings.TrimPrefix(domain, "https://"), "http://"), ".freshdesk.com")
	if baseURL == "" && domain != "" {
		baseURL = "https://" + domain + ".freshdesk.com"
	}
	return &Source{baseURL: strings.TrimRight(baseURL, "/"), key: credentials["api_key"], client: &http.Client{Timeout: 30 * time.Second}}
}

type record = map[string]any

type object struct {
	name   string
	label  string
	path   string
	since  string
	query  url.Values
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "tickets", label: "Tickets", path: "/api/v2/tickets", since: "updated_since",
		query: url.Values{"include": {"requester"}, "order_by": {"updated_at"}, "order_type": {"asc"}},
		fields: []field{
			{"id", "id", text("id")},
			{"subject", "string", text("subject")},
			{"description", "string", text("description_text")},
			{"status_id", "string", text("status")},
			{"status", "string", ticketStatus},
			{"priority", "string", ticketPriority},
			{"type", "string", text("type")},
			{"source", "string", ticketSource},
			{"requester", "id", text("requester_id")},
			{"requester_email", "email", nested("requester", "email")},
			{"requester_name", "string", nested("requester", "name")},
			{"company", "id", text("company_id")},
			{"group", "string", text("group_id")},
			{"agent", "string", text("responder_id")},
			{"tags", "string", tags("tags")},
			{"due_at", "date-time", when("due_by")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "contacts", label: "Contacts", path: "/api/v2/contacts", since: "_updated_since",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", text("phone")},
			{"mobile", "phone", text("mobile")},
			{"job_title", "string", text("job_title")},
			{"company", "id", text("company_id")},
			{"active", "boolean", boolean("active")},
			{"language", "string", text("language")},
			{"time_zone", "string", text("time_zone")},
			{"tags", "string", tags("tags")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "companies", label: "Companies", path: "/api/v2/companies", since: "",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"domain", "domain", firstOf("domains")},
			{"domains", "string", tags("domains")},
			{"description", "string", text("description")},
			{"note", "string", text("note")},
			{"industry", "string", text("industry")},
			{"health_score", "string", text("health_score")},
			{"account_tier", "string", text("account_tier")},
			{"renewal_date", "date-time", when("renewal_date")},
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
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Freshdesk has no %s to read. The objects are tickets, contacts and companies.", name)}
}

// Objects proves the key with one small call and lists the three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.baseURL == "" || s.key == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Freshdesk with its domain and API key first."}
	}
	if _, _, err := s.list(objects[1], url.Values{"per_page": {"1"}, "page": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Custom fields become columns from the records read, without their `cf_`.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	items, _, err := s.list(*object, s.listQuery(object, sampleRows, "1"))
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
	return &Description{Source: "freshdesk", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: len(rows), Hash: newestUpdated(items, ""), Counted: false}, nil
}

// Page reads one numbered page, the cursor's token or the first.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, page := 0, "", "1"
	if cursor != nil {
		offset, hash = cursor.Offset, cursor.Hash
		if cursor.Token != "" {
			page = cursor.Token
		}
	}
	items, next, err := s.list(*object, s.listQuery(object, limit, page))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(items, hash)
	answer := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return answer, nil
}

// Delta reads the records updated since the mark where the list filters
// by time, and the first page where it does not, and compares the newest.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	query := url.Values{"per_page": {strconv.Itoa(PageLimit)}, "page": {"1"}}
	if object.since != "" && mark != "" {
		query.Set(object.since, mark)
	}
	if object.name == "tickets" {
		query.Set("order_by", "updated_at")
		query.Set("order_type", "desc")
		if mark == "" {
			query.Set("updated_since", epoch)
		}
	}
	items, _, err := s.list(*object, query)
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

func (s *Source) listQuery(object *object, limit int, page string) url.Values {
	query := url.Values{"per_page": {strconv.Itoa(limit)}, "page": {page}}
	for key, values := range object.query {
		query[key] = append([]string(nil), values...)
	}
	if object.name == "tickets" {
		query.Set("updated_since", epoch)
	}
	return query
}

var nextLink = regexp.MustCompile(`<([^>]+)>;\s*rel="next"`)

// list fetches one page and answers its records and the next page number
// the Link header names, empty at the end.
func (s *Source) list(object object, query url.Values) ([]record, string, error) {
	body, status, header, err := s.get(object.path, query)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(object.name, status, body); err != nil {
		return nil, "", err
	}
	var items []record
	if err := json.Unmarshal(body, &items); err != nil {
		var wrapped struct {
			Results []record `json:"results"`
		}
		if json.Unmarshal(body, &wrapped) != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Freshdesk's answer does not parse: %s.", err)}
		}
		items = wrapped.Results
	}
	next := ""
	if match := nextLink.FindStringSubmatch(header.Get("Link")); match != nil {
		if parsed, err := url.Parse(match[1]); err == nil {
			next = parsed.Query().Get("page")
		}
	}
	return items, next, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, http.Header, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, nil, &Failure{Code: "source", Message: err.Error()}
	}
	request.SetBasicAuth(s.key, "X")
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, nil, &Failure{Code: "unreachable", Message: fmt.Sprintf("Freshdesk did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, nil, &Failure{Code: "unreachable", Message: fmt.Sprintf("Freshdesk's answer did not read: %s.", err)}
	}
	s.lastRetry = response.Header.Get("Retry-After")
	return body, response.StatusCode, response.Header, nil
}

func (s *Source) refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Freshdesk refused the API key. Connect Freshdesk again with a key that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Freshdesk refused the request for %s: %s. The agent needs access to them.", object, freshdeskMessage(body))}
	case status == http.StatusTooManyRequests:
		if seconds, err := strconv.Atoi(strings.TrimSpace(s.lastRetry)); err == nil && seconds > 0 {
			return &Failure{Code: "rate_limited", Message: fmt.Sprintf("Freshdesk asked the reader to wait %d seconds. Run again then.", seconds)}
		}
		return &Failure{Code: "rate_limited", Message: "Freshdesk asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Freshdesk answered %d: %s", status, freshdeskMessage(body))}
	}
	return nil
}

func freshdeskMessage(body []byte) string {
	var failure struct {
		Description string `json:"description"`
		Message     string `json:"message"`
		Code        string `json:"code"`
	}
	if json.Unmarshal(body, &failure) == nil {
		for _, text := range []string{failure.Description, failure.Message, failure.Code} {
			if text != "" {
				return text
			}
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a custom field key without its `cf_` prefix.
func columnName(key string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(strings.TrimPrefix(key, "cf_")), "_"), "_")
}

func customColumns(items []record, core []contract.Column) []contract.Column {
	taken := map[string]bool{}
	for _, column := range core {
		taken[column.Name] = true
	}
	seen := map[string]bool{}
	names := []string{}
	for _, item := range items {
		fields, _ := item["custom_fields"].(map[string]any)
		for key := range fields {
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

func (o *object) row(item record) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	fields, _ := item["custom_fields"].(map[string]any)
	keys := make([]string, 0, len(fields))
	for key := range fields {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		name := columnName(key)
		if _, core := row[name]; core || name == "" {
			continue
		}
		row[name] = contract.Scalar(fields[key])
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
	return func(item record) string {
		value := strings.TrimSpace(contract.Scalar(item[key]))
		if value == "" {
			return ""
		}
		if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
			return parsed.UTC().Format(time.RFC3339)
		}
		return value
	}
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// ticketStatus folds Freshdesk's numbered statuses onto the ticket kind's.
func ticketStatus(item record) string {
	switch contract.Scalar(item["status"]) {
	case "2":
		return "open"
	case "3":
		return "pending"
	case "4":
		return "resolved"
	case "5":
		return "closed"
	default:
		return ""
	}
}

func ticketPriority(item record) string {
	switch contract.Scalar(item["priority"]) {
	case "1":
		return "low"
	case "2":
		return "medium"
	case "3":
		return "high"
	case "4":
		return "urgent"
	default:
		return ""
	}
}

func ticketSource(item record) string {
	names := map[string]string{"1": "email", "2": "portal", "3": "phone", "7": "chat", "9": "feedback widget", "10": "outbound email"}
	source := contract.Scalar(item["source"])
	if name, ok := names[source]; ok {
		return name
	}
	return source
}

// newestUpdated is the change mark: the latest updated_at in a list as
// RFC 3339, the shape the updated_since filters read, or the previous mark.
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
