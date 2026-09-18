// Package airtable reads one Airtable base behind the reader contract:
// each table of the base is one object, its records are the rows, and
// every field flattens to text a mapping can point at, with the ids of the
// records a link field names as their own column. It uses the REST API
// over net/http alone, with a personal access token as the bearer, and it
// writes nothing back. Backfill walks the list through its offset token,
// and Delta is one filtered query for a record modified after the cursor's
// mark, which is the time the reader last looked.
package airtable

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Airtable's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.airtable.com"

// PageLimit is the most records one Airtable list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

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

// Source is one Airtable base reached through one personal access token.
type Source struct {
	token   string
	base    string
	baseURL string
	client  *http.Client
	tables  []table
	listed  bool
	now     func() time.Time
}

// New opens a source on the credentials the panel packs: the personal
// access token and the base id. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:   credentials["token"],
		base:    baseID(credentials["base_id"]),
		baseURL: strings.TrimRight(baseURL, "/"),
		client:  &http.Client{Timeout: 30 * time.Second},
		now:     time.Now,
	}
}

// baseID reads a base id from the id itself or from a base URL.
func baseID(value string) string {
	value = strings.TrimSpace(value)
	for _, part := range strings.Split(value, "/") {
		if strings.HasPrefix(part, "app") {
			return strings.SplitN(part, "?", 2)[0]
		}
	}
	return value
}

type record = map[string]any

// table is one table of the base as an object: its id, its name and its
// fields in the order the base shows them.
type table struct {
	id     string
	name   string
	fields []field
}

// field is one table field as a column: its id, the name the base shows,
// the column it lands in, its Airtable type, the guess for the kind schema,
// and the extra column that carries the ids of the collaborators it names.
type field struct {
	id     string
	name   string
	column string
	kind   string
	guess  string
	extra  string
}

// core is the columns every table carries before its own fields.
var core = []contract.Column{
	{Name: "id", Guess: "id"},
	{Name: "created", Guess: "date-time"},
}

// Objects reads the base schema, which proves the token, and lists every
// table.
func (s *Source) Objects() ([]ObjectInfo, error) {
	tables, err := s.schema()
	if err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(tables))
	for _, t := range tables {
		out = append(out, ObjectInfo{Name: t.id, Label: t.name})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// The list carries no count, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	t, err := s.table(name)
	if err != nil {
		return nil, err
	}
	items, _, err := s.list(t, url.Values{"pageSize": {strconv.Itoa(sampleRows)}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, t.row(entry))
	}
	return &Description{
		Source:  "airtable",
		Object:  name,
		Label:   t.name,
		Fields:  contract.Sample(t.columns(), rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    s.mark(),
		Counted: false,
	}, nil
}

// Page reads one list page after the cursor's offset token. Total is the
// rows seen so far, because the list carries no count. The mark is the
// time the first page was read, so a later Delta sees every change since.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	t, err := s.table(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset := 0
	hash := ""
	query := url.Values{"pageSize": {strconv.Itoa(limit)}}
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		if cursor.Token != "" {
			query.Set("offset", cursor.Token)
		}
	}
	if hash == "" {
		hash = s.mark()
	}
	items, next, err := s.list(t, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, t.row(entry))
	}
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks for one record whose last modified time is after the mark.
// Airtable answers no modified time on a record, so the new mark is the
// time of the check.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	t, err := s.table(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	if _, err := time.Parse(time.RFC3339, mark); err != nil {
		return &Delta{State: "changed", Hash: s.mark()}, nil
	}
	query := url.Values{
		"pageSize":        {"1"},
		"filterByFormula": {fmt.Sprintf("IS_AFTER(LAST_MODIFIED_TIME(), DATETIME_PARSE('%s'))", mark)},
	}
	items, _, err := s.list(t, query)
	if err != nil {
		return nil, err
	}
	if len(items) == 0 {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: s.mark()}, nil
}

// mark is the change mark: the reader's clock as RFC 3339 in UTC, which
// the modified time filter reads back.
func (s *Source) mark() string {
	return s.now().UTC().Format(time.RFC3339)
}

// schema reads every table of the base once per source.
func (s *Source) schema() ([]table, error) {
	if s.listed {
		return s.tables, nil
	}
	if s.token == "" || s.base == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Airtable with its personal access token and base id first."}
	}
	body, status, err := s.get("/v0/meta/bases/"+s.base+"/tables", nil)
	if err != nil {
		return nil, err
	}
	if status == http.StatusNotFound {
		return nil, &Failure{Code: "not_connected", Message: fmt.Sprintf("Airtable has no base %s the token can reach. Connect Airtable again with the base id from the base's API page.", s.base)}
	}
	if err := refusal(status, body); err != nil {
		return nil, err
	}
	var answer struct {
		Tables []struct {
			ID     string `json:"id"`
			Name   string `json:"name"`
			Fields []struct {
				ID      string         `json:"id"`
				Name    string         `json:"name"`
				Type    string         `json:"type"`
				Options map[string]any `json:"options"`
			} `json:"fields"`
		} `json:"tables"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Airtable's answer does not parse: %s.", err)}
	}
	tables := make([]table, 0, len(answer.Tables))
	for _, listed := range answer.Tables {
		t := table{id: listed.ID, name: listed.Name}
		taken := map[string]bool{}
		for _, column := range core {
			taken[column.Name] = true
		}
		for _, f := range listed.Fields {
			column := columnName(f.Name)
			if column == "" {
				continue
			}
			if taken[column] {
				column += "_field"
			}
			if taken[column] {
				continue
			}
			taken[column] = true
			entry := field{id: f.ID, name: f.Name, column: column, kind: f.Type, guess: guessType(f.Type, f.Options)}
			switch f.Type {
			case "singleCollaborator", "createdBy", "lastModifiedBy":
				entry.extra = column + "_id"
			case "multipleCollaborators":
				entry.extra = column + "_ids"
			}
			if entry.extra != "" {
				taken[entry.extra] = true
			}
			t.fields = append(t.fields, entry)
		}
		tables = append(tables, t)
	}
	s.tables = tables
	s.listed = true
	return tables, nil
}

// table finds one table by id or name.
func (s *Source) table(name string) (*table, error) {
	tables, err := s.schema()
	if err != nil {
		return nil, err
	}
	for index := range tables {
		if tables[index].id == name || tables[index].name == name {
			return &tables[index], nil
		}
	}
	names := make([]string, 0, len(tables))
	for _, t := range tables {
		names = append(names, t.name)
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Airtable has no table %s in the base. The tables are %s.", name, strings.Join(names, ", "))}
}

// columns lists the core columns, then each field and the id column beside
// it.
func (t *table) columns() []contract.Column {
	columns := append([]contract.Column{}, core...)
	for _, f := range t.fields {
		columns = append(columns, contract.Column{Name: f.column, Guess: f.guess})
		if f.extra != "" {
			guess := "id"
			if f.kind == "multipleCollaborators" {
				guess = "string"
			}
			columns = append(columns, contract.Column{Name: f.extra, Guess: guess})
		}
	}
	return columns
}

// row flattens one record: the core columns, then every field as text by
// its id, so a renamed field still lands in its column.
func (t *table) row(entry record) Row {
	row := Row{
		"id":      contract.Scalar(entry["id"]),
		"created": formatTime(contract.Scalar(entry["createdTime"])),
	}
	values, _ := entry["fields"].(map[string]any)
	for _, f := range t.fields {
		text, ids := fieldValue(f.kind, values[f.id])
		row[f.column] = text
		if f.extra != "" {
			row[f.extra] = ids
		}
	}
	return row
}

// fieldValue reads one field value as text by its type, and the ids of the
// collaborators it names when it names any.
func fieldValue(kind string, value any) (string, string) {
	switch kind {
	case "multipleSelects":
		return contract.Joined(value, true), ""
	case "multipleRecordLinks":
		return contract.Joined(value, false), ""
	case "singleCollaborator", "createdBy", "lastModifiedBy":
		return collaborator(value), collaboratorID(value)
	case "multipleCollaborators":
		list, _ := value.([]any)
		names := []string{}
		ids := []string{}
		for _, part := range list {
			if name := collaborator(part); name != "" {
				names = append(names, name)
			}
			if id := collaboratorID(part); id != "" {
				ids = append(ids, id)
			}
		}
		return strings.Join(names, ","), strings.Join(ids, ",")
	case "multipleAttachments":
		list, _ := value.([]any)
		names := []string{}
		for _, part := range list {
			typed, _ := part.(map[string]any)
			if name := contract.Scalar(typed["filename"]); name != "" {
				names = append(names, name)
			}
		}
		return strings.Join(names, ","), ""
	case "barcode":
		typed, _ := value.(map[string]any)
		return contract.Scalar(typed["text"]), ""
	case "button":
		typed, _ := value.(map[string]any)
		return contract.Scalar(typed["label"]), ""
	case "aiText":
		typed, _ := value.(map[string]any)
		return contract.Scalar(typed["value"]), ""
	case "externalSyncSource":
		typed, _ := value.(map[string]any)
		return contract.Scalar(typed["name"]), ""
	case "date":
		return contract.Scalar(value), ""
	case "dateTime", "createdTime", "lastModifiedTime":
		return formatTime(contract.Scalar(value)), ""
	case "multipleLookupValues", "formula", "rollup":
		if typed, ok := value.(map[string]any); ok {
			if state := contract.Scalar(typed["error"]); state != "" {
				return "", ""
			}
			if name := collaborator(typed); name != "" {
				return name, ""
			}
		}
		return contract.Joined(value, false), ""
	default:
		return contract.Joined(value, false), ""
	}
}

// collaborator reads a collaborator as the name it shows, else the email.
func collaborator(value any) string {
	typed, _ := value.(map[string]any)
	if name := contract.Scalar(typed["name"]); name != "" {
		return name
	}
	return contract.Scalar(typed["email"])
}

func collaboratorID(value any) string {
	typed, _ := value.(map[string]any)
	return contract.Scalar(typed["id"])
}

func guessType(kind string, options map[string]any) string {
	switch kind {
	case "number", "percent", "currency", "duration", "rating", "count", "autoNumber":
		return "number"
	case "checkbox":
		return "boolean"
	case "date":
		return "date"
	case "dateTime", "createdTime", "lastModifiedTime":
		return "date-time"
	case "email":
		return "email"
	case "phoneNumber":
		return "phone"
	case "singleCollaborator", "createdBy", "lastModifiedBy":
		return "string"
	case "formula", "rollup", "multipleLookupValues":
		result, _ := options["result"].(map[string]any)
		if inner := contract.Scalar(result["type"]); inner != "" && inner != kind {
			return guessType(inner, nil)
		}
		return "string"
	default:
		return "string"
	}
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a field's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// formatTime reads an Airtable timestamp as RFC 3339 in UTC, and leaves a
// plain date as it stands.
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

// list fetches one page of a table's records by field id and answers them
// with the next offset token, empty when the collection ends.
func (s *Source) list(t *table, query url.Values) ([]record, string, error) {
	query.Set("returnFieldsByFieldId", "true")
	body, status, err := s.get("/v0/"+s.base+"/"+t.id, query)
	if err != nil {
		return nil, "", err
	}
	if status == http.StatusNotFound {
		return nil, "", &Failure{Code: "unknown_object", Message: fmt.Sprintf("Airtable has no table %s in the base.", t.name)}
	}
	if err := refusal(status, body); err != nil {
		return nil, "", err
	}
	var listed struct {
		Records []record `json:"records"`
		Offset  string   `json:"offset"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Airtable's answer does not parse: %s.", err)}
	}
	return listed.Records, listed.Offset, nil
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
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Airtable did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Airtable's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Airtable answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Airtable refused the personal access token. Connect Airtable again with a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Airtable refused the token for the base: %s. The token needs the data.records:read and schema.bases:read scopes and access to the base.", airtableMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Airtable asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Airtable answered %d: %s", status, airtableMessage(body))}
	}
	return nil
}

// airtableMessage reads the error Airtable writes, which is a string or an
// object with a type and a message.
func airtableMessage(body []byte) string {
	var failure struct {
		Error any `json:"error"`
	}
	if json.Unmarshal(body, &failure) == nil {
		switch typed := failure.Error.(type) {
		case string:
			return typed
		case map[string]any:
			if message := contract.Scalar(typed["message"]); message != "" {
				return message
			}
			return contract.Scalar(typed["type"])
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}
