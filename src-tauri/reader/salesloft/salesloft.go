// Package salesloft reads a Salesloft team behind the reader contract:
// three objects, people, accounts and cadences, each flattened to text
// fields a mapping can point at. It uses the v2 REST API over net/http
// alone, with the API key as the bearer, and it writes nothing back.
// Backfill walks the numbered pages, and Delta is one list filtered to
// records updated after the cursor's mark.
package salesloft

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

// DefaultBaseURL is Salesloft's API host. A test points the reader
// elsewhere.
const DefaultBaseURL = "https://api.salesloft.com"

// PageLimit is the most records one Salesloft list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// customFieldLimit caps the custom fields one row carries.
const customFieldLimit = 200

// markLayout is how the change mark is written, with the milliseconds
// Salesloft keeps, so the updated_at filter reads it back.
const markLayout = "2006-01-02T15:04:05.000000Z"

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

// Source is one Salesloft team.
type Source struct {
	token  string
	base   string
	client *http.Client
	custom map[string][]customField
	listed bool
}

// customField is one custom field as a column: the name the record
// carries it under in custom_fields and the column name.
type customField struct {
	key    string
	column contract.Column
}

// New opens a source on an API key. An empty base URL is the live API.
func New(token, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:  token,
		base:   strings.TrimRight(baseURL, "/"),
		client: &http.Client{Timeout: 30 * time.Second},
		custom: map[string][]customField{},
	}
}

type record = map[string]any

// object is one Salesloft list: its path, the custom field type that
// rides on it, and the core fields in the order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	custom string
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
		name:   "people",
		label:  "People",
		path:   "/v2/people.json",
		custom: "person",
		fields: []field{
			{"id", "id", text("id")},
			{"first_name", "string", text("first_name")},
			{"last_name", "string", text("last_name")},
			{"name", "string", personName},
			{"email", "email", text("email_address")},
			{"email_domain", "domain", emailDomain("email_address")},
			{"phone", "phone", text("phone")},
			{"mobile", "phone", text("mobile_phone")},
			{"job_title", "string", text("title")},
			{"company_name", "string", text("person_company_name")},
			{"account_id", "id", refID("account")},
			{"owner_id", "id", refID("owner")},
			{"stage_id", "id", refID("person_stage")},
			{"do_not_contact", "boolean", boolean("do_not_contact")},
			{"tags", "string", tags("tags")},
			{"last_contacted_at", "date-time", when("last_contacted_at")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name:   "accounts",
		label:  "Accounts",
		path:   "/v2/accounts.json",
		custom: "company",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"domain", "domain", text("domain")},
			{"website", "string", text("website")},
			{"industry", "string", text("industry")},
			{"type", "string", text("company_type")},
			{"size", "string", text("size")},
			{"city", "string", text("city")},
			{"state", "string", text("state")},
			{"country", "string", text("country")},
			{"owner_id", "id", refID("owner")},
			{"stage_id", "id", refID("company_stage")},
			{"tags", "string", tags("tags")},
			{"archived_at", "date-time", when("archived_at")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name:  "cadences",
		label: "Cadences",
		path:  "/v2/cadences.json",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"type", "string", text("cadence_type")},
			{"state", "string", text("current_state")},
			{"shared", "boolean", boolean("shared")},
			{"team_cadence", "boolean", boolean("team_cadence")},
			{"owner_id", "id", refID("owner")},
			{"tags", "string", tags("tags")},
			{"archived_at", "date-time", when("archived_at")},
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
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Salesloft has no %s to read. The objects are people, accounts and cadences.", name)}
}

// Objects lists the three objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap call proves the key before the panel offers the objects.
	if _, _, _, err := s.list(objects[0].path, url.Values{"per_page": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. The list names its total when asked, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	items, total, _, err := s.list(object.path, pageQuery(sampleRows, 1))
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
	if total < len(rows) {
		total = len(rows)
	}
	return &Description{
		Source:  "salesloft",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    total,
		Bytes:   0,
		Hash:    newestUpdated(items, ""),
		Counted: true,
	}, nil
}

// Page reads one numbered page from the cursor's page. Total is the
// list's own count.
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
	custom := s.customFields(object)
	items, total, next, err := s.list(object.path, pageQuery(limit, page))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	hash = newestUpdated(items, hash)
	if total < offset+len(rows) {
		total = offset + len(rows)
	}
	answer := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: true}
	if next > 0 {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(next)}
	}
	return answer, nil
}

// Delta asks for the one record updated after the mark, newest first.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	query := url.Values{"per_page": {"1"}, "sort_by": {"updated_at"}, "sort_direction": {"DESC"}}
	if mark != "" {
		query.Set("updated_at[gt]", mark)
	}
	items, _, _, err := s.list(object.path, query)
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

// pageQuery builds one numbered page with its counts.
func pageQuery(limit, page int) url.Values {
	return url.Values{
		"per_page":              {strconv.Itoa(limit)},
		"page":                  {strconv.Itoa(page)},
		"include_paging_counts": {"true"},
	}
}

// customFields reads the team's custom field definitions once per source
// and keeps every field of the object's type as a column named as the
// team named it. A key that cannot read them reads as having none,
// because the core columns still land.
func (s *Source) customFields(object *object) []customField {
	if object.custom == "" {
		return nil
	}
	if !s.listed {
		s.listed = true
		s.readCustomFields()
	}
	return s.custom[object.custom]
}

func (s *Source) readCustomFields() {
	taken := map[string]map[string]bool{}
	for _, object := range objects {
		if object.custom == "" {
			continue
		}
		taken[object.custom] = map[string]bool{}
		for _, f := range object.fields {
			taken[object.custom][f.name] = true
		}
	}
	body, status, err := s.get("/v2/custom_fields.json", url.Values{"per_page": {"100"}})
	if err != nil || status != http.StatusOK {
		return
	}
	var answer struct {
		Data []struct {
			Name      string `json:"name"`
			FieldType string `json:"field_type"`
		} `json:"data"`
	}
	if json.Unmarshal(body, &answer) != nil {
		return
	}
	for _, definition := range answer.Data {
		names, ok := taken[definition.FieldType]
		if !ok {
			continue
		}
		name := columnName(definition.Name)
		if name == "" || names[name] {
			continue
		}
		names[name] = true
		s.custom[definition.FieldType] = append(s.custom[definition.FieldType], customField{key: definition.Name, column: contract.Column{Name: name, Guess: "string"}})
	}
	for kind := range s.custom {
		listed := s.custom[kind]
		sort.Slice(listed, func(a, b int) bool { return listed[a].column.Name < listed[b].column.Name })
		if len(listed) > customFieldLimit {
			listed = listed[:customFieldLimit]
		}
		s.custom[kind] = listed
	}
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a field's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// list fetches one page of a list endpoint and answers its records, the
// total count when the answer names one, and the next page number, zero
// when the collection ends.
func (s *Source) list(path string, query url.Values) ([]record, int, int, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, 0, 0, err
	}
	if err := refusal(status, body); err != nil {
		return nil, 0, 0, err
	}
	var listed struct {
		Data     []record `json:"data"`
		Metadata struct {
			Paging struct {
				NextPage   *int `json:"next_page"`
				TotalCount *int `json:"total_count"`
			} `json:"paging"`
		} `json:"metadata"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, 0, 0, &Failure{Code: "source", Message: fmt.Sprintf("Salesloft's answer does not parse: %s.", err)}
	}
	next, total := 0, 0
	if listed.Metadata.Paging.NextPage != nil {
		next = *listed.Metadata.Paging.NextPage
	}
	if listed.Metadata.Paging.TotalCount != nil {
		total = *listed.Metadata.Paging.TotalCount
	}
	return listed.Data, total, next, nil
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
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Salesloft did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Salesloft's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Salesloft answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Salesloft refused the API key. Connect Salesloft again with a key that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Salesloft refused the key: %s", salesloftMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Salesloft asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Salesloft answered %d: %s", status, salesloftMessage(body))}
	}
	return nil
}

func salesloftMessage(body []byte) string {
	var failure struct {
		Error  string `json:"error"`
		Errors any    `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if failure.Error != "" {
			return failure.Error
		}
		if failure.Errors != nil {
			return contract.Scalar(failure.Errors)
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record: the core fields, then every custom field as
// its own column named as the team named it.
func (o *object) row(item record, custom []customField) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	values, _ := item["custom_fields"].(map[string]any)
	for _, extra := range custom {
		row[extra.column.Name] = contract.Joined(values[extra.key], true)
	}
	return row
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

// refID reads the id of an embedded reference such as account or owner.
func refID(key string) func(record) string {
	return func(item record) string {
		if reference, ok := item[key].(map[string]any); ok {
			return contract.Scalar(reference["id"])
		}
		return ""
	}
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

func personName(item record) string {
	if name := strings.TrimSpace(contract.Scalar(item["display_name"])); name != "" {
		return name
	}
	name := strings.TrimSpace(strings.TrimSpace(contract.Scalar(item["first_name"])) + " " + strings.TrimSpace(contract.Scalar(item["last_name"])))
	if name != "" {
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

// formatTime reads Salesloft's ISO timestamp as RFC 3339 in UTC.
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
// its microseconds, so the updated_at filter reads it back, or the
// previous mark when nothing newer appears.
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
