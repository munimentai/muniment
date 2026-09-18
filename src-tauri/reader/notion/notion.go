// Package notion reads a Notion workspace behind the reader contract: every
// database the integration can see is one object, its pages are the
// records, and every property flattens to text a mapping can point at. It
// uses the public REST API over net/http alone, with an internal
// integration token as the bearer, and it writes nothing back. Backfill
// walks the database query through its start cursor, and Delta is one
// query for a page edited after the cursor's mark.
package notion

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"sort"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Notion's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.notion.com"

// Version is the API version every request names.
const Version = "2022-06-28"

// PageLimit is the most pages one Notion query returns.
const PageLimit = 100

// sampleRows is how many pages Describe reads for its samples.
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

// Source is one Notion workspace reached through one integration.
type Source struct {
	token     string
	baseURL   string
	client    *http.Client
	databases map[string]*database
}

// New opens a source on an internal integration token. An empty base URL
// is the live API.
func New(token, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:     token,
		baseURL:   strings.TrimRight(baseURL, "/"),
		client:    &http.Client{Timeout: 30 * time.Second},
		databases: map[string]*database{},
	}
}

type item = map[string]any

// database is one Notion database as an object: its id, its title and its
// properties in the order Describe lists them.
type database struct {
	id         string
	title      string
	properties []property
}

// property is one database property as a column: the name Notion shows,
// the column it lands in, its Notion type, the guess for the kind schema,
// and the extra column that carries the ids of the people it names.
type property struct {
	name   string
	column string
	kind   string
	guess  string
	extra  string
}

// core is the columns every database carries before its own properties.
var core = []contract.Column{
	{Name: "id", Guess: "id"},
	{Name: "name", Guess: "string"},
	{Name: "url", Guess: "string"},
	{Name: "parent", Guess: "id"},
	{Name: "created_by", Guess: "id"},
	{Name: "modified_by", Guess: "id"},
	{Name: "created", Guess: "date-time"},
	{Name: "modified", Guess: "date-time"},
	{Name: "archived", Guess: "boolean"},
}

// Objects walks the search for every database the integration can see.
// The first search call proves the token.
func (s *Source) Objects() ([]ObjectInfo, error) {
	out := []ObjectInfo{}
	cursor := ""
	for {
		body := item{"filter": item{"property": "object", "value": "database"}, "page_size": PageLimit}
		if cursor != "" {
			body["start_cursor"] = cursor
		}
		results, next, err := s.post("/v1/search", body)
		if err != nil {
			return nil, err
		}
		for _, result := range results {
			db := readDatabase(result)
			s.databases[db.id] = db
			out = append(out, ObjectInfo{Name: db.id, Label: db.title})
		}
		if next == "" {
			break
		}
		cursor = next
	}
	sort.Slice(out, func(a, b int) bool { return out[a].Label < out[b].Label })
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// A query carries no count, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	db, err := s.database(name)
	if err != nil {
		return nil, err
	}
	items, _, err := s.post("/v1/databases/"+db.id+"/query", item{"page_size": sampleRows})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, db.row(entry))
	}
	return &Description{
		Source:  "notion",
		Object:  name,
		Label:   db.title,
		Fields:  contract.Sample(db.columns(), rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestEdit(items, ""),
		Counted: false,
	}, nil
}

// Page reads one query page after the cursor's token. Total is the rows
// seen so far, because the query carries no count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	db, err := s.database(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset := 0
	hash := ""
	body := item{"page_size": limit}
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		if cursor.Token != "" {
			body["start_cursor"] = cursor.Token
		}
	}
	items, next, err := s.post("/v1/databases/"+db.id+"/query", body)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, db.row(entry))
	}
	hash = newestEdit(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks the query for one page edited after the cursor's mark, newest
// first, and answers the newest edit it saw.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	db, err := s.database(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	body := item{
		"page_size": 1,
		"sorts":     []item{{"timestamp": "last_edited_time", "direction": "descending"}},
	}
	if mark != "" {
		body["filter"] = item{"timestamp": "last_edited_time", "last_edited_time": item{"after": mark}}
	}
	items, _, err := s.post("/v1/databases/"+db.id+"/query", body)
	if err != nil {
		return nil, err
	}
	if len(items) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestEdit(items, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// database finds one database by id: from the search Objects ran, else by
// one read of the database itself.
func (s *Source) database(name string) (*database, error) {
	if cached, ok := s.databases[name]; ok {
		return cached, nil
	}
	body, status, err := s.get("/v1/databases/" + name)
	if err != nil {
		return nil, err
	}
	if status == http.StatusNotFound {
		return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Notion has no database %s the integration can see. Share the database with the integration.", name)}
	}
	if err := refusal(status, body); err != nil {
		return nil, err
	}
	var result item
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Notion's answer does not parse: %s.", err)}
	}
	db := readDatabase(result)
	s.databases[name] = db
	return db, nil
}

// readDatabase reads one database object: its title and its properties as
// columns, named as the workspace named them and sorted by name, with the
// title property left out because the name column carries it.
func readDatabase(result item) *database {
	db := &database{id: contract.Scalar(result["id"]), title: plainText(result["title"])}
	if db.title == "" {
		db.title = "Untitled"
	}
	taken := map[string]bool{}
	for _, column := range core {
		taken[column.Name] = true
	}
	schema, _ := result["properties"].(map[string]any)
	names := make([]string, 0, len(schema))
	for name := range schema {
		names = append(names, name)
	}
	sort.Strings(names)
	for _, name := range names {
		definition, _ := schema[name].(map[string]any)
		kind := contract.Scalar(definition["type"])
		if kind == "title" || kind == "button" {
			continue
		}
		column := columnName(name)
		if column == "" {
			continue
		}
		if taken[column] {
			column += "_property"
		}
		if taken[column] {
			continue
		}
		taken[column] = true
		p := property{name: name, column: column, kind: kind, guess: guessType(kind, definition)}
		switch kind {
		case "people":
			p.extra = column + "_ids"
		case "created_by", "last_edited_by":
			p.extra = column + "_id"
		}
		if p.extra != "" {
			taken[p.extra] = true
		}
		db.properties = append(db.properties, p)
	}
	return db
}

// columns lists the core columns, then each property and the id column
// beside it.
func (d *database) columns() []contract.Column {
	columns := append([]contract.Column{}, core...)
	for _, p := range d.properties {
		columns = append(columns, contract.Column{Name: p.column, Guess: p.guess})
		if p.extra != "" {
			guess := "id"
			if p.kind == "people" {
				guess = "string"
			}
			columns = append(columns, contract.Column{Name: p.extra, Guess: guess})
		}
	}
	return columns
}

// row flattens one page: the core columns, then every property as text and
// the ids of the people it names as their own column.
func (d *database) row(entry item) Row {
	row := Row{
		"id":          contract.Scalar(entry["id"]),
		"url":         contract.Scalar(entry["url"]),
		"parent":      parentID(entry),
		"created_by":  userID(entry["created_by"]),
		"modified_by": userID(entry["last_edited_by"]),
		"created":     formatTime(contract.Scalar(entry["created_time"])),
		"modified":    formatTime(contract.Scalar(entry["last_edited_time"])),
		"archived":    archived(entry),
	}
	values, _ := entry["properties"].(map[string]any)
	row["name"] = ""
	for _, raw := range values {
		value, _ := raw.(map[string]any)
		if contract.Scalar(value["type"]) == "title" {
			row["name"] = plainText(value["title"])
		}
	}
	for _, p := range d.properties {
		value, _ := values[p.name].(map[string]any)
		text, ids := propertyValue(value)
		row[p.column] = text
		if p.extra != "" {
			row[p.extra] = ids
		}
	}
	return row
}

// propertyValue reads one property value as text, and the ids of the
// people it names when it names any.
func propertyValue(value map[string]any) (string, string) {
	kind := contract.Scalar(value["type"])
	inner := value[kind]
	switch kind {
	case "title", "rich_text":
		return plainText(inner), ""
	case "number", "url", "email", "phone_number", "checkbox":
		return contract.Scalar(inner), ""
	case "select", "status":
		return nameOf(inner), ""
	case "multi_select":
		return namesOf(inner, true), ""
	case "date":
		return dateStart(inner), ""
	case "people":
		return namesOf(inner, false), idsOf(inner)
	case "created_by", "last_edited_by":
		return nameOf(inner), userID(inner)
	case "created_time", "last_edited_time":
		return formatTime(contract.Scalar(inner)), ""
	case "relation":
		return idsOf(inner), ""
	case "files":
		return fileNames(inner), ""
	case "formula", "rollup":
		return computed(inner), ""
	case "unique_id":
		return uniqueID(inner), ""
	case "verification":
		typed, _ := inner.(map[string]any)
		return contract.Scalar(typed["state"]), ""
	default:
		return "", ""
	}
}

// computed reads a formula or rollup by the type of its result.
func computed(inner any) string {
	typed, _ := inner.(map[string]any)
	switch contract.Scalar(typed["type"]) {
	case "string":
		return contract.Scalar(typed["string"])
	case "number":
		return contract.Scalar(typed["number"])
	case "boolean":
		return contract.Scalar(typed["boolean"])
	case "date":
		return dateStart(typed["date"])
	case "array":
		list, _ := typed["array"].([]any)
		parts := []string{}
		for _, part := range list {
			element, _ := part.(map[string]any)
			if text, _ := propertyValue(element); text != "" {
				parts = append(parts, text)
			}
		}
		return strings.Join(parts, ",")
	default:
		return ""
	}
}

func guessType(kind string, definition map[string]any) string {
	switch kind {
	case "number":
		return "number"
	case "checkbox":
		return "boolean"
	case "date":
		return "date"
	case "created_time", "last_edited_time":
		return "date-time"
	case "email":
		return "email"
	case "phone_number":
		return "phone"
	case "rollup":
		rollup, _ := definition["rollup"].(map[string]any)
		switch contract.Scalar(rollup["function"]) {
		case "count", "count_values", "sum", "average", "median", "min", "max", "range", "percent_empty", "percent_not_empty":
			return "number"
		}
		return "string"
	default:
		return "string"
	}
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a property's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// plainText joins a rich text list as the text it shows.
func plainText(value any) string {
	list, ok := value.([]any)
	if !ok {
		return ""
	}
	var text strings.Builder
	for _, part := range list {
		span, _ := part.(map[string]any)
		text.WriteString(contract.Scalar(span["plain_text"]))
	}
	return strings.TrimSpace(text.String())
}

func nameOf(value any) string {
	typed, _ := value.(map[string]any)
	return contract.Scalar(typed["name"])
}

func namesOf(value any, sorted bool) string {
	list, _ := value.([]any)
	names := []string{}
	for _, part := range list {
		if name := nameOf(part); name != "" {
			names = append(names, name)
		}
	}
	if sorted {
		sort.Strings(names)
	}
	return strings.Join(names, ",")
}

func idsOf(value any) string {
	list, _ := value.([]any)
	ids := []string{}
	for _, part := range list {
		typed, _ := part.(map[string]any)
		if id := contract.Scalar(typed["id"]); id != "" {
			ids = append(ids, id)
		}
	}
	return strings.Join(ids, ",")
}

func userID(value any) string {
	typed, _ := value.(map[string]any)
	return contract.Scalar(typed["id"])
}

func dateStart(value any) string {
	typed, _ := value.(map[string]any)
	return formatTime(contract.Scalar(typed["start"]))
}

func fileNames(value any) string {
	list, _ := value.([]any)
	names := []string{}
	for _, part := range list {
		typed, _ := part.(map[string]any)
		if name := contract.Scalar(typed["name"]); name != "" {
			names = append(names, name)
		}
	}
	return strings.Join(names, ",")
}

func uniqueID(value any) string {
	typed, _ := value.(map[string]any)
	number := contract.Scalar(typed["number"])
	if prefix := contract.Scalar(typed["prefix"]); prefix != "" && number != "" {
		return prefix + "-" + number
	}
	return number
}

func parentID(entry item) string {
	parent, _ := entry["parent"].(map[string]any)
	return contract.Scalar(parent["database_id"])
}

func archived(entry item) string {
	switch value := entry["archived"].(type) {
	case bool:
		if value {
			return "yes"
		}
		return "no"
	default:
		return ""
	}
}

// formatTime reads a Notion timestamp as RFC 3339 in UTC, and leaves a
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

// post runs one query or search and answers its results and the next
// start cursor, empty when the collection ends.
func (s *Source) post(path string, body item) ([]item, string, error) {
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, "", &Failure{Code: "source", Message: err.Error()}
	}
	request, err := http.NewRequest(http.MethodPost, s.baseURL+path, bytes.NewReader(encoded))
	if err != nil {
		return nil, "", &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/json")
	answer, status, err := s.do(request)
	if err != nil {
		return nil, "", err
	}
	if status == http.StatusNotFound && strings.HasPrefix(path, "/v1/databases/") {
		return nil, "", &Failure{Code: "unknown_object", Message: "Notion has no such database the integration can see. Share the database with the integration."}
	}
	if err := refusal(status, answer); err != nil {
		return nil, "", err
	}
	var listed struct {
		Results    []item `json:"results"`
		HasMore    bool   `json:"has_more"`
		NextCursor string `json:"next_cursor"`
	}
	if err := json.Unmarshal(answer, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Notion's answer does not parse: %s.", err)}
	}
	next := ""
	if listed.HasMore {
		next = listed.NextCursor
	}
	return listed.Results, next, nil
}

func (s *Source) get(path string) ([]byte, int, error) {
	request, err := http.NewRequest(http.MethodGet, s.baseURL+path, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Notion-Version", Version)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Notion did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Notion's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Notion answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Notion refused the integration token. Connect Notion again with an internal integration token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Notion refused the request: %s. The integration needs the read content capability.", notionMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Notion asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Notion answered %d: %s", status, notionMessage(body))}
	}
	return nil
}

func notionMessage(body []byte) string {
	var failure struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Message != "" {
		return failure.Message
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// newestEdit is the change mark: the latest last_edited_time in a list as
// RFC 3339, so the query filter reads it back, or the previous mark when
// nothing newer appears.
func newestEdit(items []item, previous string) string {
	var newest time.Time
	for _, entry := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(entry["last_edited_time"])); err == nil && parsed.After(newest) {
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
