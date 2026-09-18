// Package kit reads a Kit account behind the reader contract: four objects,
// subscribers, tags, forms and sequences, each flattened to text fields a
// mapping can point at. It uses the v4 REST API over net/http alone, with
// the API key in its own header, and it writes nothing back. Backfill walks
// each list through its end cursor. Delta is one subscribers list filtered
// to records updated after the cursor's mark, and the hash of the first
// page for the lists Kit cannot filter by change time.
package kit

import (
	"crypto/sha256"
	"encoding/hex"
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

// DefaultBaseURL is Kit's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.kit.com"

// PageLimit is the most records one Kit list call returns.
const PageLimit = 500

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// customFieldLimit caps the custom fields one row carries.
const customFieldLimit = 200

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

// Source is one Kit account reached through one API key.
type Source struct {
	key    string
	base   string
	client *http.Client
	custom []customField
	listed bool
	now    func() time.Time
}

// customField is one custom field as a column: the label the subscriber
// carries it under in fields and the column name.
type customField struct {
	label  string
	column contract.Column
}

// New opens a source on an API key. An empty base URL is the live API.
func New(key, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		key:    key,
		base:   strings.TrimRight(baseURL, "/"),
		client: &http.Client{Timeout: 30 * time.Second},
		now:    time.Now,
	}
}

type record = map[string]any

// object is one Kit list: its path, the key its records sit under in the
// answer, the query every read passes, and the fields in the order
// Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	key    string
	query  url.Values
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
		name:  "subscribers",
		label: "Subscribers",
		path:  "/v4/subscribers",
		key:   "subscribers",
		query: url.Values{"status": {"all"}},
		fields: []field{
			{"id", "id", text("id")},
			{"email", "email", text("email_address")},
			{"email_domain", "domain", emailDomain("email_address")},
			{"first_name", "string", text("first_name")},
			{"name", "string", subscriberName},
			{"state", "string", text("state")},
			{"created", "date-time", when("created_at")},
		},
	},
	{
		name:  "tags",
		label: "Tags",
		path:  "/v4/tags",
		key:   "tags",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"created", "date-time", when("created_at")},
		},
	},
	{
		name:  "forms",
		label: "Forms",
		path:  "/v4/forms",
		key:   "forms",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"type", "string", text("type")},
			{"format", "string", text("format")},
			{"uid", "string", text("uid")},
			{"embed_url", "string", text("embed_url")},
			{"archived", "boolean", boolean("archived")},
			{"created", "date-time", when("created_at")},
		},
	},
	{
		name:  "sequences",
		label: "Sequences",
		path:  "/v4/sequences",
		key:   "sequences",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"hold", "boolean", boolean("hold")},
			{"repeat", "boolean", boolean("repeat")},
			{"created", "date-time", when("created_at")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Kit has no %s to read. The objects are subscribers, tags, forms and sequences.", name)}
}

// filtered is true for the one object Kit filters by change time.
func (o *object) filtered() bool {
	return o.name == "subscribers"
}

// Objects lists the four objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap call proves the key before the panel offers the objects.
	if _, _, err := s.list(objects[1].path, objects[1].key, url.Values{"per_page": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. A list carries no count, so Rows is the rows read and Counted
// is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	asked := s.mark()
	items, _, err := s.list(object.path, object.key, object.page(sampleRows, ""))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	hash := hashRows(rows)
	if object.filtered() {
		hash = asked
	}
	return &Description{
		Source:  "kit",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(object.columns(custom), rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    hash,
		Counted: false,
	}, nil
}

// Page reads one page after the cursor's token. Total is the rows seen so
// far, because a list carries no count. The subscribers mark is the time
// the first page was asked for, because a subscriber carries no change
// time of its own and the updated_after filter reads the clock back. The
// other marks are the hash of the first page. Later pages carry the mark
// forward.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, token := 0, "", ""
	if cursor != nil {
		offset, hash, token = cursor.Offset, cursor.Hash, cursor.Token
	}
	custom := s.customFields(object)
	asked := s.mark()
	items, next, err := s.list(object.path, object.key, object.page(limit, token))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	if token == "" {
		hash = hashRows(rows)
		if object.filtered() {
			hash = asked
		}
	}
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks for one subscriber updated after the mark and answers the
// time it asked as the new mark. For the other objects it reads the first
// page again and compares its hash with the cursor's.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	if !object.filtered() {
		custom := s.customFields(object)
		items, _, err := s.list(object.path, object.key, object.page(PageLimit, ""))
		if err != nil {
			return nil, err
		}
		rows := make([]Row, 0, len(items))
		for _, item := range items {
			rows = append(rows, object.row(item, custom))
		}
		hash := hashRows(rows)
		if hash == mark {
			return &Delta{State: "unchanged"}, nil
		}
		return &Delta{State: "changed", Hash: hash}, nil
	}
	asked := s.mark()
	query := object.page(1, "")
	if mark != "" {
		query.Set("updated_after", mark)
	}
	items, _, err := s.list(object.path, object.key, query)
	if err != nil {
		return nil, err
	}
	if len(items) == 0 && mark != "" {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: asked}, nil
}

// mark is the subscribers change mark: the clock as RFC 3339 in UTC, taken
// before the request so a change during it shows next time.
func (s *Source) mark() string {
	return s.now().UTC().Format(time.RFC3339)
}

// page builds one cursor page with the object's own query.
func (o *object) page(limit int, after string) url.Values {
	query := url.Values{"per_page": {strconv.Itoa(limit)}}
	for key, values := range o.query {
		query[key] = values
	}
	if after != "" {
		query.Set("after", after)
	}
	return query
}

// customFields reads the account's custom field definitions once per
// source and keeps every one as a column on subscribers named as the
// account named it. A key that cannot read them reads as having none,
// because the core columns still land.
func (s *Source) customFields(object *object) []customField {
	if !object.filtered() {
		return nil
	}
	if !s.listed {
		s.listed = true
		s.readCustomFields(object)
	}
	return s.custom
}

func (s *Source) readCustomFields(object *object) {
	body, status, err := s.get("/v4/custom_fields", url.Values{"per_page": {strconv.Itoa(PageLimit)}})
	if err != nil || status != http.StatusOK {
		return
	}
	var answer struct {
		CustomFields []struct {
			Key   string `json:"key"`
			Label string `json:"label"`
		} `json:"custom_fields"`
	}
	if json.Unmarshal(body, &answer) != nil {
		return
	}
	taken := map[string]bool{}
	for _, f := range object.fields {
		taken[f.name] = true
	}
	for _, definition := range answer.CustomFields {
		name := columnName(definition.Label)
		if name == "" {
			name = columnName(definition.Key)
		}
		if name == "" || taken[name] {
			continue
		}
		taken[name] = true
		s.custom = append(s.custom, customField{label: definition.Label, column: contract.Column{Name: name, Guess: "string"}})
	}
	sort.Slice(s.custom, func(a, b int) bool { return s.custom[a].column.Name < s.custom[b].column.Name })
	if len(s.custom) > customFieldLimit {
		s.custom = s.custom[:customFieldLimit]
	}
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a field's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// list fetches one page of a list endpoint and answers the records under
// the key and the end cursor of the page, empty when the collection ends.
func (s *Source) list(path, key string, query url.Values) ([]record, string, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, "", err
	}
	if err := refusal(status, body); err != nil {
		return nil, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Kit's answer does not parse: %s.", err)}
	}
	var pagination struct {
		HasNextPage bool   `json:"has_next_page"`
		EndCursor   string `json:"end_cursor"`
	}
	if raw, ok := listed["pagination"]; ok {
		if err := json.Unmarshal(raw, &pagination); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Kit's answer does not parse: %s.", err)}
		}
	}
	items := []record{}
	if raw, ok := listed[key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Kit's answer does not parse: %s.", err)}
		}
	}
	next := ""
	if pagination.HasNextPage {
		next = pagination.EndCursor
	}
	return items, next, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	target := s.base + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("X-Kit-Api-Key", s.key)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Kit did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Kit's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Kit answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Kit refused the API key. Connect Kit again with a key that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Kit refused the request: %s. Connect Kit again with a key from an account whose plan opens the API.", kitMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Kit asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Kit answered %d: %s", status, kitMessage(body))}
	}
	return nil
}

func kitMessage(body []byte) string {
	var failure struct {
		Errors []string `json:"errors"`
		Error  string   `json:"error"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if len(failure.Errors) > 0 {
			return strings.Join(failure.Errors, ", ")
		}
		if failure.Error != "" {
			return failure.Error
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// columns lists the object's fields, then the custom fields.
func (o *object) columns(custom []customField) []contract.Column {
	columns := make([]contract.Column, 0, len(o.fields)+len(custom))
	for _, f := range o.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	for _, extra := range custom {
		columns = append(columns, extra.column)
	}
	return columns
}

// row flattens one record: the core fields, then every custom field as
// its own column named as the account named it.
func (o *object) row(item record, custom []customField) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	values, _ := item["fields"].(map[string]any)
	for _, extra := range custom {
		row[extra.column.Name] = contract.Scalar(values[extra.label])
	}
	return row
}

// hashRows is the change mark for the lists Kit cannot filter: the
// SHA-256 of the rows as JSON.
func hashRows(rows []Row) string {
	if rows == nil {
		rows = []Row{}
	}
	encoded, _ := json.Marshal(rows)
	sum := sha256.Sum256(encoded)
	return hex.EncodeToString(sum[:])
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
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

func subscriberName(item record) string {
	if name := strings.TrimSpace(contract.Scalar(item["first_name"])); name != "" {
		return name
	}
	return contract.Scalar(item["email_address"])
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

// formatTime reads Kit's ISO timestamp as RFC 3339 in UTC.
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
