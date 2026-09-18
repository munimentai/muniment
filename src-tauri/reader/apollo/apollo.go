// Package apollo reads an Apollo.io team behind the reader contract: two
// objects, contacts and accounts, each flattened to text fields a mapping
// can point at. It uses the v1 REST API over net/http alone, with the API
// key in the x-api-key header, and it writes nothing back. Backfill walks
// the numbered pages of the search endpoints, and Delta is one search
// sorted newest change first, read against the cursor's mark.
package apollo

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Apollo's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.apollo.io"

// PageLimit is the most records one Apollo search call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// markLayout is how the change mark is written, with the milliseconds
// Apollo keeps, so two marks compare as the timestamps they name.
const markLayout = "2006-01-02T15:04:05.000Z"

// The wire shapes come from the contract package, so the runtime reads one
// set of fields whatever the source.
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

// Source is one Apollo team.
type Source struct {
	key    string
	base   string
	client *http.Client
}

// New opens a source on an API key. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		key:    contract.Credentials(secret)["token"],
		base:   strings.TrimRight(baseURL, "/"),
		client: &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one Apollo search: its path, the key its records answer
// under, the sort field that orders by change time, and the core fields
// in the order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	key    string
	sort   string
	fields []field
}

// field names one text column, how the kind schema should type it, and
// how it reads out of a record.
type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name:  "contacts",
		label: "Contacts",
		path:  "/api/v1/contacts/search",
		key:   "contacts",
		sort:  "contact_updated_at",
		fields: []field{
			{"id", "id", text("id")},
			{"first_name", "string", text("first_name")},
			{"last_name", "string", text("last_name")},
			{"name", "string", contactName},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain("email")},
			{"email_status", "string", text("email_status")},
			{"phone", "phone", contactPhone},
			{"job_title", "string", text("title")},
			{"company_name", "string", text("organization_name")},
			{"account_id", "id", text("account_id")},
			{"organization_id", "id", text("organization_id")},
			{"owner_id", "id", text("owner_id")},
			{"stage_id", "id", text("contact_stage_id")},
			{"linkedin_url", "string", text("linkedin_url")},
			{"city", "string", text("city")},
			{"state", "string", text("state")},
			{"country", "string", text("country")},
			{"labels", "string", tags("label_ids")},
			{"last_activity_at", "date-time", when("last_activity_date")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name:  "accounts",
		label: "Accounts",
		path:  "/api/v1/accounts/search",
		key:   "accounts",
		sort:  "account_updated_at",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"domain", "domain", text("domain")},
			{"website", "string", text("website_url")},
			{"phone", "phone", accountPhone},
			{"industry", "string", text("industry")},
			{"linkedin_url", "string", text("linkedin_url")},
			{"organization_id", "id", text("organization_id")},
			{"owner_id", "id", text("owner_id")},
			{"stage_id", "id", text("account_stage_id")},
			{"city", "string", text("city")},
			{"state", "string", text("state")},
			{"country", "string", text("country")},
			{"labels", "string", tags("label_ids")},
			{"last_activity_at", "date-time", when("last_activity_date")},
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
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Apollo has no %s to read. The objects are contacts and accounts.", name)}
}

// Objects lists the two objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap search proves the key before the panel offers the objects.
	if _, _, _, err := s.search(&objects[0], pageBody(1, 1, "", true)); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. The search names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	items, total, _, err := s.search(object, pageBody(sampleRows, 1, object.sort, true))
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
	if total < len(rows) {
		total = len(rows)
	}
	return &Description{
		Source:  "apollo",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    total,
		Bytes:   0,
		Hash:    newestUpdated(items, ""),
		Counted: true,
	}, nil
}

// Page reads one numbered page from the cursor's page, oldest change
// first so a record changed during the walk moves behind the reader.
// Total is the search's own count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, page := 0, "", 1
	if cursor != nil {
		offset, hash = cursor.Offset, cursor.Hash
		if parsed, err := strconv.Atoi(cursor.Token); err == nil && parsed > 1 {
			page = parsed
		}
	}
	items, total, pages, err := s.search(object, pageBody(limit, page, object.sort, true))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(items, hash)
	if total < offset+len(rows) {
		total = offset + len(rows)
	}
	answer := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: true}
	if page < pages && len(rows) > 0 {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(page + 1)}
	}
	return answer, nil
}

// Delta asks for the one record changed most recently. Apollo cannot
// filter a search by change time, so the mark is that record's updated_at
// and a newer one than the cursor's mark reads as changed.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	items, _, _, err := s.search(object, pageBody(1, 1, object.sort, false))
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

// pageBody builds one numbered page of a search, sorted by the field
// named when one is.
func pageBody(limit, page int, sort string, ascending bool) map[string]any {
	body := map[string]any{"per_page": limit, "page": page}
	if sort != "" {
		body["sort_by_field"] = sort
		body["sort_ascending"] = ascending
	}
	return body
}

// search posts one page of a search endpoint and answers its records, the
// total count, and the count of pages.
func (s *Source) search(object *object, body map[string]any) ([]record, int, int, error) {
	answer, status, err := s.post(object.path, body)
	if err != nil {
		return nil, 0, 0, err
	}
	if err := refusal(status, answer); err != nil {
		return nil, 0, 0, err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(answer, &listed); err != nil {
		return nil, 0, 0, &Failure{Code: "source", Message: fmt.Sprintf("Apollo's answer does not parse: %s.", err)}
	}
	items := []record{}
	if raw, ok := listed[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, 0, 0, &Failure{Code: "source", Message: fmt.Sprintf("Apollo's %s do not parse: %s.", object.key, err)}
		}
	}
	var pagination struct {
		TotalEntries int `json:"total_entries"`
		TotalPages   int `json:"total_pages"`
	}
	if raw, ok := listed["pagination"]; ok {
		_ = json.Unmarshal(raw, &pagination)
	}
	return items, pagination.TotalEntries, pagination.TotalPages, nil
}

func (s *Source) post(path string, body map[string]any) ([]byte, int, error) {
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request, err := http.NewRequest(http.MethodPost, s.base+path, bytes.NewReader(encoded))
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("x-api-key", s.key)
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Cache-Control", "no-cache")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Apollo did not answer: %s.", err)}
	}
	defer response.Body.Close()
	answer, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Apollo's answer did not read: %s.", err)}
	}
	return answer, response.StatusCode, nil
}

// refusal turns a status Apollo answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Apollo refused the API key. Connect Apollo again with a key that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Apollo refused the key: %s. Connect Apollo again with a key whose plan reads the object.", apolloMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Apollo asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Apollo answered %d: %s", status, apolloMessage(body))}
	}
	return nil
}

func apolloMessage(body []byte) string {
	var failure struct {
		Error        string `json:"error"`
		Message      string `json:"message"`
		ErrorMessage string `json:"error_message"`
	}
	if json.Unmarshal(body, &failure) == nil {
		for _, text := range []string{failure.ErrorMessage, failure.Error, failure.Message} {
			if text != "" {
				return text
			}
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record to the core fields.
func (o *object) row(item record) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	return row
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

func tags(key string) func(record) string {
	return func(item record) string { return contract.Joined(item[key], true) }
}

func contactName(item record) string {
	if name := strings.TrimSpace(contract.Scalar(item["name"])); name != "" {
		return name
	}
	name := strings.TrimSpace(strings.TrimSpace(contract.Scalar(item["first_name"])) + " " + strings.TrimSpace(contract.Scalar(item["last_name"])))
	if name != "" {
		return name
	}
	return contract.Scalar(item["email"])
}

// contactPhone reads the first number of phone_numbers, sanitized when
// Apollo sanitized it, else the contact's own sanitized phone.
func contactPhone(item record) string {
	if numbers, ok := item["phone_numbers"].([]any); ok {
		for _, entry := range numbers {
			number, ok := entry.(map[string]any)
			if !ok {
				continue
			}
			if phone := contract.Scalar(number["sanitized_number"]); phone != "" {
				return phone
			}
			if phone := contract.Scalar(number["raw_number"]); phone != "" {
				return phone
			}
		}
	}
	return contract.Scalar(item["sanitized_phone"])
}

// accountPhone reads the sanitized phone, else the raw one.
func accountPhone(item record) string {
	if phone := contract.Scalar(item["sanitized_phone"]); phone != "" {
		return phone
	}
	return contract.Scalar(item["phone"])
}

func emailDomain(key string) func(record) string {
	return func(item record) string {
		email := contract.Scalar(item[key])
		if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
			return strings.ToLower(email[at+1:])
		}
		return ""
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Apollo's ISO timestamp as RFC 3339 in UTC.
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

// newestUpdated is the change mark: the latest updated_at in a list with
// its milliseconds, or the previous mark when nothing newer appears.
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
	return newest.UTC().Format(markLayout)
}
