// Package zoominfo reads a ZoomInfo account behind the reader contract:
// two objects, contacts and companies, each flattened to text fields a
// mapping can point at. It uses the REST API over net/http alone, trades
// the username and password for a JWT once per process, and writes
// nothing back. Backfill walks the numbered pages of the search
// endpoints, and Delta is one search for records updated after the
// cursor's mark, newest first.
package zoominfo

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

// DefaultBaseURL is ZoomInfo's API host. A test points the reader
// elsewhere.
const DefaultBaseURL = "https://api.zoominfo.com"

// PageLimit is the most records one ZoomInfo search call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// markLayout is how the change mark is written, so two marks compare as
// the timestamps they name.
const markLayout = "2006-01-02T15:04:05Z"

// dayLayout is how the lastUpdatedDateAfter filter reads a date.
const dayLayout = "2006-01-02"

// timeLayouts are the shapes ZoomInfo writes a timestamp in.
var timeLayouts = []string{time.RFC3339Nano, "1/2/2006 3:04:05 PM", "2006-01-02 15:04:05", dayLayout}

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

// Source is one ZoomInfo account.
type Source struct {
	username string
	password string
	base     string
	client   *http.Client
	token    string
}

// New opens a source on the credentials the panel packs: the username and
// the password. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		username: credentials["username"],
		password: credentials["password"],
		base:     strings.TrimRight(baseURL, "/"),
		client:   &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one ZoomInfo search: its path and the core fields in the
// order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
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
		path:  "/search/contact",
		fields: []field{
			{"id", "id", text("id")},
			{"first_name", "string", text("firstName")},
			{"last_name", "string", text("lastName")},
			{"name", "string", contactName},
			{"job_title", "string", text("jobTitle")},
			{"company_id", "id", nested("company", "id")},
			{"company_name", "string", nested("company", "name")},
			{"has_email", "boolean", boolean("hasEmail")},
			{"has_phone", "boolean", boolean("hasPhone")},
			{"accuracy_score", "number", text("contactAccuracyScore")},
			{"valid_at", "date-time", when("validDate")},
			{"modified", "date-time", when("lastUpdatedDate")},
		},
	},
	{
		name:  "companies",
		label: "Companies",
		path:  "/search/company",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"website", "string", text("website")},
			{"domain", "domain", websiteDomain("website")},
			{"employees", "number", text("employeeCount")},
			{"revenue", "number", text("revenue")},
			{"industry", "string", text("primaryIndustry")},
			{"modified", "date-time", when("lastUpdatedDate")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("ZoomInfo has no %s to read. The objects are contacts and companies.", name)}
}

// Objects trades the password for a JWT, which proves the account, and
// lists the two objects.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if err := s.signIn(); err != nil {
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
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, total, _, err := s.search(object, pageBody(sampleRows, 1, "asc"))
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
		Source:  "zoominfo",
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
	if err := s.signIn(); err != nil {
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
	items, total, reachable, err := s.search(object, pageBody(limit, page, "asc"))
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
	if len(rows) > 0 && offset+len(rows) < total && (reachable == 0 || offset+len(rows) < reachable) {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(page + 1)}
	}
	return answer, nil
}

// Delta asks for the one record updated most recently on or after the
// mark's day, newest first. The filter reads a day, so a record updated
// earlier on the mark's own day reads back and compares as older.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	body := pageBody(1, 1, "desc")
	if marked, err := time.Parse(time.RFC3339Nano, mark); err == nil {
		body["lastUpdatedDateAfter"] = marked.UTC().Format(dayLayout)
	}
	items, _, _, err := s.search(object, body)
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

// pageBody builds one numbered page of a search, ordered by change time
// in the direction named.
func pageBody(limit, page int, order string) map[string]any {
	return map[string]any{"rpp": limit, "page": page, "sortBy": "lastUpdatedDate", "sortOrder": order}
}

// signIn trades the username and password for a JWT once per process.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.username == "" || s.password == "" {
		return &Failure{Code: "not_connected", Message: "Connect ZoomInfo with its username and password first."}
	}
	body, status, err := s.post("/authenticate", map[string]any{"username": s.username, "password": s.password}, false)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusForbidden || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("ZoomInfo refused the sign-in: %s. Connect ZoomInfo again with the username and password of an API user.", zoominfoMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("ZoomInfo answered %d at sign-in: %s", status, zoominfoMessage(body))}
	}
	var granted struct {
		JWT string `json:"jwt"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.JWT == "" {
		return &Failure{Code: "source", Message: "ZoomInfo's sign-in answer carried no JWT."}
	}
	s.token = granted.JWT
	return nil
}

// search posts one page of a search endpoint and answers its records, the
// total count, and the count the search can reach when it caps one.
func (s *Source) search(object *object, body map[string]any) ([]record, int, int, error) {
	answer, status, err := s.post(object.path, body, true)
	if err != nil {
		return nil, 0, 0, err
	}
	if err := refusal(status, answer); err != nil {
		return nil, 0, 0, err
	}
	var listed struct {
		MaxResults   int      `json:"maxResults"`
		TotalResults int      `json:"totalResults"`
		Data         []record `json:"data"`
	}
	if err := json.Unmarshal(answer, &listed); err != nil {
		return nil, 0, 0, &Failure{Code: "source", Message: fmt.Sprintf("ZoomInfo's answer does not parse: %s.", err)}
	}
	return listed.Data, listed.TotalResults, listed.MaxResults, nil
}

func (s *Source) post(path string, body map[string]any, signed bool) ([]byte, int, error) {
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request, err := http.NewRequest(http.MethodPost, s.base+path, bytes.NewReader(encoded))
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	if signed {
		request.Header.Set("Authorization", "Bearer "+s.token)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("ZoomInfo did not answer: %s.", err)}
	}
	defer response.Body.Close()
	answer, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("ZoomInfo's answer did not read: %s.", err)}
	}
	return answer, response.StatusCode, nil
}

// refusal turns a status ZoomInfo answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "ZoomInfo refused the JWT. Connect ZoomInfo again with the username and password of an API user."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("ZoomInfo refused the request: %s. Connect ZoomInfo again with a user whose plan reads the object.", zoominfoMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "ZoomInfo asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("ZoomInfo answered %d: %s", status, zoominfoMessage(body))}
	}
	return nil
}

func zoominfoMessage(body []byte) string {
	var failure struct {
		Message string `json:"message"`
		Error   string `json:"error"`
		Data    struct {
			Error string `json:"error"`
		} `json:"data"`
	}
	if json.Unmarshal(body, &failure) == nil {
		for _, text := range []string{failure.Message, failure.Data.Error, failure.Error} {
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

// nested reads one key of an embedded object such as company.
func nested(key, inner string) func(record) string {
	return func(item record) string {
		if embedded, ok := item[key].(map[string]any); ok {
			return contract.Scalar(embedded[inner])
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

func contactName(item record) string {
	return strings.TrimSpace(strings.TrimSpace(contract.Scalar(item["firstName"])) + " " + strings.TrimSpace(contract.Scalar(item["lastName"])))
}

// websiteDomain reads the host of a website, without its scheme or a
// leading www.
func websiteDomain(key string) func(record) string {
	return func(item record) string {
		site := strings.ToLower(strings.TrimSpace(contract.Scalar(item[key])))
		if site == "" {
			return ""
		}
		if at := strings.Index(site, "://"); at >= 0 {
			site = site[at+3:]
		}
		if slash := strings.Index(site, "/"); slash >= 0 {
			site = site[:slash]
		}
		return strings.TrimPrefix(site, "www.")
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// parseTime reads a timestamp in any shape ZoomInfo writes, as UTC.
func parseTime(value string) (time.Time, bool) {
	value = strings.TrimSpace(value)
	for _, layout := range timeLayouts {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed.UTC(), true
		}
	}
	return time.Time{}, false
}

// formatTime reads ZoomInfo's timestamp as RFC 3339 in UTC.
func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if parsed, ok := parseTime(value); ok {
		return parsed.Format(time.RFC3339)
	}
	return value
}

// newestUpdated is the change mark: the latest lastUpdatedDate in a list,
// or the previous mark when nothing newer appears.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, ok := parseTime(contract.Scalar(item["lastUpdatedDate"])); ok && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, ok := parseTime(previous); ok && before.After(newest) {
		return previous
	}
	return newest.Format(markLayout)
}
