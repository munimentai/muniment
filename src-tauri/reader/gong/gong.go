// Package gong reads a Gong instance behind the reader contract: two
// objects, users and calls, each flattened to text fields a mapping can
// point at. It uses the v2 REST API over net/http alone, with the access
// key and its secret as basic auth, and it writes nothing back. Backfill
// walks the cursor pages. Delta on calls is one list from the cursor's
// mark, and Delta on users is the hash of the first page, because Gong
// cannot filter users by change time.
package gong

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Gong's shared API host. The secret's api_domain names
// the instance's own host, and a test points the reader elsewhere.
const DefaultBaseURL = "https://api.gong.io"

// PageLimit is how many records Gong puts on one page. Gong sizes the
// page itself, so the limit a caller asks for is not sent.
const PageLimit = 100

// markLayout is how a call mark is written, so the fromDateTime filter
// reads it back.
const markLayout = "2006-01-02T15:04:05Z"

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

// Source is one Gong instance reached through one access key.
type Source struct {
	key    string
	secret string
	base   string
	client *http.Client
}

// New opens a source on the credentials the panel packs: the access key,
// the access key secret and the API domain. An empty base URL is the
// secret's API domain, else the shared host.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		baseURL = credentials["api_domain"]
	}
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		key:    credentials["access_key"],
		secret: credentials["access_key_secret"],
		base:   strings.TrimRight(baseURL, "/"),
		client: &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one Gong list: its path, the key its records answer under,
// the time field that marks a change when the list filters by one, and
// the core fields in the order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	key    string
	mark   string
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
		name:  "users",
		label: "Users",
		path:  "/v2/users",
		key:   "users",
		fields: []field{
			{"id", "id", text("id")},
			{"first_name", "string", text("firstName")},
			{"last_name", "string", text("lastName")},
			{"name", "string", userName},
			{"email", "email", text("emailAddress")},
			{"email_domain", "domain", emailDomain("emailAddress")},
			{"email_aliases", "string", tags("emailAliases")},
			{"job_title", "string", text("title")},
			{"phone", "phone", text("phoneNumber")},
			{"extension", "string", text("extension")},
			{"active", "boolean", boolean("active")},
			{"manager_id", "id", text("managerId")},
			{"created", "date-time", when("created")},
		},
	},
	{
		name:  "calls",
		label: "Calls",
		path:  "/v2/calls",
		key:   "calls",
		mark:  "started",
		fields: []field{
			{"id", "id", text("id")},
			{"title", "string", text("title")},
			{"url", "string", text("url")},
			{"direction", "string", text("direction")},
			{"system", "string", text("system")},
			{"scope", "string", text("scope")},
			{"media", "string", text("media")},
			{"language", "string", text("language")},
			{"purpose", "string", text("purpose")},
			{"duration_seconds", "number", text("duration")},
			{"private", "boolean", boolean("isPrivate")},
			{"primary_user_id", "id", text("primaryUserId")},
			{"workspace_id", "id", text("workspaceId")},
			{"client_unique_id", "string", text("clientUniqueId")},
			{"calendar_event_id", "string", text("calendarEventId")},
			{"meeting_url", "string", text("meetingUrl")},
			{"sdr_disposition", "string", text("sdrDisposition")},
			{"scheduled", "date-time", when("scheduled")},
			{"started", "date-time", when("started")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Gong has no %s to read. The objects are users and calls.", name)}
}

// Objects lists the two objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap call proves the key before the panel offers the objects.
	if _, _, err := s.get("/v2/workspaces", nil); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. The list names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	items, total, _, err := s.list(object, nil)
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
		Source:  "gong",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    total,
		Bytes:   0,
		Hash:    object.changeMark(items, ""),
		Counted: true,
	}, nil
}

// Page reads one list page after the cursor's token. Total is the list's
// own count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	offset, hash, after := 0, "", ""
	if cursor != nil {
		offset, hash, after = cursor.Offset, cursor.Hash, cursor.Token
	}
	query := url.Values{}
	if after != "" {
		query.Set("cursor", after)
	}
	items, total, next, err := s.list(object, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = object.changeMark(items, hash)
	if total < offset+len(rows) {
		total = offset + len(rows)
	}
	page := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: true}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks for the calls started at or after the mark and reads the
// newest start on that page. A call at the mark itself answers unchanged.
// Users have no time filter, so the first page is hashed and read against
// the mark.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	query := url.Values{}
	if object.mark != "" && mark != "" {
		query.Set("fromDateTime", mark)
	}
	items, _, _, err := s.list(object, query)
	if err != nil {
		return nil, err
	}
	if object.mark == "" {
		hash := hashRecords(items)
		if hash == mark {
			return &Delta{State: "unchanged"}, nil
		}
		return &Delta{State: "changed", Hash: hash}, nil
	}
	if len(items) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestTime(items, object.mark, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// changeMark is the object's change mark after a page: the newest start
// seen for calls, and for users the hash of the first page, which later
// pages keep.
func (o *object) changeMark(items []record, previous string) string {
	if o.mark == "" {
		if previous != "" {
			return previous
		}
		return hashRecords(items)
	}
	return newestTime(items, o.mark, previous)
}

// list fetches one page of a list endpoint and answers its records, the
// total count, and the cursor of the next page, empty when the collection
// ends. Gong answers 404 to a call filter that matches nothing, and that
// reads as an empty list.
func (s *Source) list(object *object, query url.Values) ([]record, int, string, error) {
	body, status, err := s.get(object.path, query)
	if err != nil {
		if failure, ok := err.(*Failure); ok && status == http.StatusNotFound && strings.Contains(strings.ToLower(failure.Message), "no calls found") {
			return []record{}, 0, "", nil
		}
		return nil, 0, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, 0, "", &Failure{Code: "source", Message: fmt.Sprintf("Gong's answer does not parse: %s.", err)}
	}
	items := []record{}
	if raw, ok := listed[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, 0, "", &Failure{Code: "source", Message: fmt.Sprintf("Gong's %s do not parse: %s.", object.key, err)}
		}
	}
	var records struct {
		TotalRecords int    `json:"totalRecords"`
		Cursor       string `json:"cursor"`
	}
	if raw, ok := listed["records"]; ok {
		_ = json.Unmarshal(raw, &records)
	}
	return items, records.TotalRecords, records.Cursor, nil
}

// get fetches one path and answers its body and status, or the failure a
// refused or failed status reads as.
func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	if s.key == "" || s.secret == "" {
		return nil, 0, &Failure{Code: "not_connected", Message: "Connect Gong with its access key, access key secret and API domain first."}
	}
	target := s.base + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.SetBasicAuth(s.key, s.secret)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Gong did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Gong's answer did not read: %s.", err)}
	}
	if err := refusal(response.StatusCode, body); err != nil {
		return nil, response.StatusCode, err
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Gong answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Gong refused the access key. Connect Gong again with the access key and secret the Gong API page shows."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Gong refused the request: %s. Connect Gong again with a key whose scopes read the object.", gongMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Gong asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Gong answered %d: %s", status, gongMessage(body))}
	}
	return nil
}

func gongMessage(body []byte) string {
	var failure struct {
		Errors []string `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Errors) > 0 {
		return strings.Join(failure.Errors, "; ")
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

func userName(item record) string {
	name := strings.TrimSpace(strings.TrimSpace(contract.Scalar(item["firstName"])) + " " + strings.TrimSpace(contract.Scalar(item["lastName"])))
	if name != "" {
		return name
	}
	return contract.Scalar(item["emailAddress"])
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

// formatTime reads Gong's ISO timestamp, which carries the instance's
// offset, as RFC 3339 in UTC.
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

// newestTime is the latest value of a time field in a list, in UTC, or
// the previous mark when nothing newer appears.
func newestTime(items []record, key, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item[key])); err == nil && parsed.After(newest) {
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

// hashRecords is the change mark of a list with no time filter: the
// SHA-256 of its records as JSON.
func hashRecords(items []record) string {
	if items == nil {
		items = []record{}
	}
	encoded, _ := json.Marshal(items)
	sum := sha256.Sum256(encoded)
	return hex.EncodeToString(sum[:])
}
