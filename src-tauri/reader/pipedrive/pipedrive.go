// Package pipedrive reads a Pipedrive company behind the reader contract:
// three objects, persons, organizations and deals, each flattened to text
// fields a mapping can point at. It uses the v1 REST API over net/http
// alone, with the API token in a header and never in a URL, and it writes
// nothing back. Backfill walks the paged list endpoints, and Delta asks the
// recents endpoint for one change after the cursor's mark.
package pipedrive

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

// DefaultBaseURL is Pipedrive's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.pipedrive.com"

// PageLimit is the most items one Pipedrive list call returns.
const PageLimit = 100

// sampleRows is how many items Describe reads for its samples.
const sampleRows = 100

// timeLayout is how Pipedrive writes every timestamp, in UTC.
const timeLayout = "2006-01-02 15:04:05"

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

// Source is one Pipedrive company.
type Source struct {
	token   string
	baseURL string
	client  *http.Client
	custom  map[string][]property
	stages  map[string]stage
	staged  bool
}

// New opens a source on an API token. An empty base URL is the live API.
func New(token, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:   token,
		baseURL: strings.TrimRight(baseURL, "/"),
		client:  &http.Client{Timeout: 30 * time.Second},
		custom:  map[string][]property{},
	}
}

type item = map[string]any

// object is one Pipedrive list: its path, the fields endpoint that names
// its custom fields, the recents item name, and the core fields in the
// order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	fields string
	recent string
	core   []field
}

// field names one text column, how the kind schema should type it, and
// how it reads out of an item.
type field struct {
	name  string
	guess string
	read  func(*Source, item) string
}

// property is one custom field the company defines, as a column: the
// 40-character key Pipedrive stores it under and the name it shows.
type property struct {
	key   string
	name  string
	guess string
}

// stage is one deal stage: its pipeline and its position in it, so a deal
// folds onto the deal kind's stages by how far along it stands.
type stage struct {
	name     string
	pipeline string
	position int
	count    int
}

var objects = []object{
	{
		name:   "persons",
		label:  "People",
		path:   "/v1/persons",
		fields: "/v1/personFields",
		recent: "person",
		core: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"first_name", "string", text("first_name")},
			{"last_name", "string", text("last_name")},
			{"email", "email", primary("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", primary("phone")},
			{"job_title", "string", text("job_title")},
			{"organization", "id", refID("org_id")},
			{"organization_name", "string", refName("org_id")},
			{"owner", "string", refName("owner_id")},
			{"label", "string", text("label")},
			{"open_deals", "number", text("open_deals_count")},
			{"created", "date-time", when("add_time")},
			{"modified", "date-time", when("update_time")},
			{"active", "boolean", boolean("active_flag")},
		},
	},
	{
		name:   "organizations",
		label:  "Organizations",
		path:   "/v1/organizations",
		fields: "/v1/organizationFields",
		recent: "organization",
		core: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"address", "string", text("address")},
			{"city", "string", text("address_locality")},
			{"country", "string", text("address_country")},
			{"owner", "string", refName("owner_id")},
			{"label", "string", text("label")},
			{"people", "number", text("people_count")},
			{"open_deals", "number", text("open_deals_count")},
			{"won_deals", "number", text("won_deals_count")},
			{"created", "date-time", when("add_time")},
			{"modified", "date-time", when("update_time")},
			{"active", "boolean", boolean("active_flag")},
		},
	},
	{
		name:   "deals",
		label:  "Deals",
		path:   "/v1/deals",
		fields: "/v1/dealFields",
		recent: "deal",
		core: []field{
			{"id", "id", text("id")},
			{"name", "string", text("title")},
			{"amount", "number", text("value")},
			{"currency", "string", text("currency")},
			{"status", "string", text("status")},
			{"stage_id", "string", text("stage_id")},
			{"stage_name", "string", stageName},
			{"stage", "string", dealStage},
			{"pipeline", "string", text("pipeline_id")},
			{"person", "id", refID("person_id")},
			{"organization", "id", refID("org_id")},
			{"owner", "string", refName("user_id")},
			{"expected_close", "date", text("expected_close_date")},
			{"won_at", "date-time", when("won_time")},
			{"lost_at", "date-time", when("lost_time")},
			{"lost_reason", "string", text("lost_reason")},
			{"created", "date-time", when("add_time")},
			{"modified", "date-time", when("update_time")},
			{"active", "boolean", boolean("active")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Pipedrive has no %s to read. The objects are persons, organizations and deals.", name)}
}

// Objects lists the three objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap call proves the token before the panel offers the objects.
	if _, _, err := s.list(objects[0].path, url.Values{"limit": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Pipedrive's list carries no count, so Rows is the rows read and Counted
// is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	items, _, err := s.list(object.path, url.Values{"limit": {strconv.Itoa(sampleRows)}, "start": {"0"}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, object.row(s, entry, custom))
	}
	columns := make([]property, 0, len(object.core)+len(custom))
	for _, f := range object.core {
		columns = append(columns, property{name: f.name, guess: f.guess})
	}
	columns = append(columns, custom...)
	fields := make([]FieldDescription, 0, len(columns))
	for _, column := range columns {
		var samples []string
		seen := map[string]bool{}
		filled := 0
		for _, row := range rows {
			value := row[column.name]
			if value == "" {
				continue
			}
			filled++
			if len(samples) < 3 && !seen[value] {
				seen[value] = true
				samples = append(samples, clip(value, 80))
			}
		}
		if samples == nil {
			samples = []string{}
		}
		fields = append(fields, FieldDescription{Name: column.name, Guess: column.guess, Samples: samples, Filled: filled})
	}
	return &Description{
		Source:  "pipedrive",
		Object:  name,
		Label:   object.label,
		Fields:  fields,
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestUpdate(items, ""),
		Counted: false,
	}, nil
}

// Page reads one list page from the cursor's start. Total is the rows seen
// so far, because the list carries no count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset := 0
	hash := ""
	start := "0"
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		if cursor.Token != "" {
			start = cursor.Token
		}
	}
	custom := s.customFields(object)
	items, next, err := s.list(object.path, url.Values{"limit": {strconv.Itoa(limit)}, "start": {start}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, entry := range items {
		rows = append(rows, object.row(s, entry, custom))
	}
	hash = newestUpdate(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks the recents endpoint for one item of the object changed after
// the cursor's mark. Without a mark it reads the newest change from the
// list sorted by update time.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	if mark == "" {
		items, _, err := s.list(object.path, url.Values{"limit": {"1"}, "start": {"0"}, "sort": {"update_time DESC"}})
		if err != nil {
			return nil, err
		}
		return &Delta{State: "changed", Hash: newestUpdate(items, "")}, nil
	}
	body, status, err := s.get("/v1/recents", url.Values{
		"since_timestamp": {mark},
		"items":           {object.recent},
		"start":           {"0"},
		"limit":           {strconv.Itoa(PageLimit)},
	})
	if err != nil {
		return nil, err
	}
	if err := s.refusal(status, body); err != nil {
		return nil, err
	}
	var recent struct {
		Data []struct {
			Data item `json:"data"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &recent); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Pipedrive's answer does not parse: %s.", err)}
	}
	items := make([]item, 0, len(recent.Data))
	for _, entry := range recent.Data {
		if entry.Data != nil {
			items = append(items, entry.Data)
		}
	}
	newest := newestUpdate(items, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// customFields reads the company's own fields on an object once per
// source. A field Pipedrive defines is a core column or nothing.
func (s *Source) customFields(object *object) []property {
	if cached, ok := s.custom[object.name]; ok {
		return cached
	}
	taken := map[string]bool{}
	for _, f := range object.core {
		taken[f.name] = true
	}
	listed := []property{}
	body, status, err := s.get(object.fields, url.Values{"limit": {"500"}})
	if err == nil && status == http.StatusOK {
		var answer struct {
			Data []struct {
				Key       string `json:"key"`
				Name      string `json:"name"`
				FieldType string `json:"field_type"`
				EditFlag  bool   `json:"edit_flag"`
			} `json:"data"`
		}
		if json.Unmarshal(body, &answer) == nil {
			for _, result := range answer.Data {
				if !result.EditFlag || len(result.Key) != 40 || result.Name == "" {
					continue
				}
				name := columnName(result.Name)
				if name == "" || taken[name] {
					continue
				}
				taken[name] = true
				listed = append(listed, property{key: result.Key, name: name, guess: guessType(result.FieldType)})
			}
		}
	}
	sort.Slice(listed, func(a, b int) bool { return listed[a].name < listed[b].name })
	s.custom[object.name] = listed
	return listed
}

// dealStages reads every stage once, so a deal folds by its position.
func (s *Source) dealStages() map[string]stage {
	if s.staged {
		return s.stages
	}
	s.staged = true
	s.stages = map[string]stage{}
	body, status, err := s.get("/v1/stages", url.Values{})
	if err != nil || status != http.StatusOK {
		return s.stages
	}
	var answer struct {
		Data []struct {
			ID         any    `json:"id"`
			Name       string `json:"name"`
			PipelineID any    `json:"pipeline_id"`
			OrderNr    int    `json:"order_nr"`
		} `json:"data"`
	}
	if json.Unmarshal(body, &answer) != nil {
		return s.stages
	}
	counts := map[string]int{}
	for _, entry := range answer.Data {
		counts[scalar(entry.PipelineID)]++
	}
	// Positions count from one in each pipeline's own order.
	sort.Slice(answer.Data, func(a, b int) bool { return answer.Data[a].OrderNr < answer.Data[b].OrderNr })
	positions := map[string]int{}
	for _, entry := range answer.Data {
		pipeline := scalar(entry.PipelineID)
		positions[pipeline]++
		s.stages[scalar(entry.ID)] = stage{name: entry.Name, pipeline: pipeline, position: positions[pipeline], count: counts[pipeline]}
	}
	return s.stages
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a field's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

func guessType(fieldType string) string {
	switch fieldType {
	case "int", "double", "monetary":
		return "number"
	case "date":
		return "date"
	case "daterange", "time", "timerange":
		return "string"
	default:
		return "string"
	}
}

// list fetches one page of a list endpoint and answers its data and the
// next start, empty when the collection ends.
func (s *Source) list(path string, query url.Values) ([]item, string, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(status, body); err != nil {
		return nil, "", err
	}
	var listed struct {
		Success        bool   `json:"success"`
		Data           []item `json:"data"`
		AdditionalData struct {
			Pagination struct {
				More      bool `json:"more_items_in_collection"`
				NextStart int  `json:"next_start"`
			} `json:"pagination"`
		} `json:"additional_data"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Pipedrive's answer does not parse: %s.", err)}
	}
	if !listed.Success {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Pipedrive answered %d: %s", status, pipedriveMessage(body))}
	}
	next := ""
	if listed.AdditionalData.Pagination.More {
		next = strconv.Itoa(listed.AdditionalData.Pagination.NextStart)
	}
	return listed.Data, next, nil
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
	request.Header.Set("x-api-token", s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Pipedrive did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Pipedrive's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Pipedrive answers into the one failure the
// runtime reads.
func (s *Source) refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Pipedrive refused the API token. Connect Pipedrive again with a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Pipedrive refused the token: %s", pipedriveMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Pipedrive asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Pipedrive answered %d: %s", status, pipedriveMessage(body))}
	}
	return nil
}

func pipedriveMessage(body []byte) string {
	var failure struct {
		Error     string `json:"error"`
		ErrorInfo string `json:"error_info"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error != "" {
		return failure.Error
	}
	return clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one item: the core fields, then every custom field as its
// own column named as the company named it.
func (o *object) row(s *Source, entry item, custom []property) Row {
	row := Row{}
	for _, f := range o.core {
		row[f.name] = f.read(s, entry)
	}
	for _, column := range custom {
		row[column.name] = customValue(entry[column.key])
	}
	return row
}

// customValue reads a custom field's value: a scalar as text, a monetary
// or address object by its shown value, a set of options joined.
func customValue(value any) string {
	switch typed := value.(type) {
	case map[string]any:
		if shown := scalar(typed["value"]); shown != "" {
			return shown
		}
		if shown := scalar(typed["formatted_address"]); shown != "" {
			return shown
		}
		return scalar(typed["name"])
	case []any:
		parts := []string{}
		for _, part := range typed {
			if text := customValue(part); text != "" {
				parts = append(parts, text)
			}
		}
		return strings.Join(parts, ",")
	default:
		return scalar(value)
	}
}

func text(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string { return scalar(entry[key]) }
}

// primary reads the primary value of a person's email or phone list, else
// the first.
func primary(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string {
		list, ok := entry[key].([]any)
		if !ok {
			return scalar(entry[key])
		}
		first := ""
		for _, part := range list {
			contact, _ := part.(map[string]any)
			value := scalar(contact["value"])
			if value == "" {
				continue
			}
			if isTrue(contact["primary"]) {
				return value
			}
			if first == "" {
				first = value
			}
		}
		return first
	}
}

func emailDomain(s *Source, entry item) string {
	email := primary("email")(s, entry)
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// refID reads a related record's id from an id or an expanded object.
func refID(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string {
		switch value := entry[key].(type) {
		case map[string]any:
			if id := scalar(value["value"]); id != "" {
				return id
			}
			return scalar(value["id"])
		default:
			return scalar(value)
		}
	}
}

func refName(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string {
		if value, ok := entry[key].(map[string]any); ok {
			return scalar(value["name"])
		}
		return scalar(entry[key])
	}
}

func boolean(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string {
		switch value := entry[key].(type) {
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

func isTrue(value any) bool {
	switch typed := value.(type) {
	case bool:
		return typed
	case string:
		return typed == "true" || typed == "1"
	case float64:
		return typed == 1
	}
	return false
}

// when reads a Pipedrive timestamp as RFC 3339 in UTC.
func when(key string) func(*Source, item) string {
	return func(_ *Source, entry item) string { return formatTime(scalar(entry[key])) }
}

func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" || strings.HasPrefix(value, "0000") {
		return ""
	}
	if parsed, err := time.Parse(timeLayout, value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	return value
}

func stageName(s *Source, entry item) string {
	return s.dealStages()[scalar(entry["stage_id"])].name
}

// dealStage folds a deal onto the deal kind's stages: won and lost by
// status, and an open deal by how far along its pipeline it stands, the
// first stage discovery, the last negotiation, and the ones between split
// across qualification and proposal.
func dealStage(s *Source, entry item) string {
	switch scalar(entry["status"]) {
	case "won":
		return "won"
	case "lost":
		return "lost"
	case "deleted":
		return ""
	}
	found, ok := s.dealStages()[scalar(entry["stage_id"])]
	if !ok || found.count == 0 {
		return ""
	}
	if found.count == 1 {
		return "qualification"
	}
	switch {
	case found.position == 1:
		return "discovery"
	case found.position == found.count:
		return "negotiation"
	case float64(found.position-1)/float64(found.count-1) < 0.5:
		return "qualification"
	default:
		return "proposal"
	}
}

func scalar(value any) string {
	switch typed := value.(type) {
	case nil:
		return ""
	case string:
		return typed
	case bool:
		if typed {
			return "yes"
		}
		return "no"
	case float64:
		if typed == float64(int64(typed)) {
			return strconv.FormatInt(int64(typed), 10)
		}
		return strconv.FormatFloat(typed, 'f', -1, 64)
	default:
		encoded, err := json.Marshal(typed)
		if err != nil {
			return ""
		}
		return string(encoded)
	}
}

// newestUpdate is the change mark: the latest update_time in a list, in
// Pipedrive's own layout so the recents endpoint reads it back, or the
// previous mark when nothing newer appears.
func newestUpdate(items []item, previous string) string {
	var newest time.Time
	for _, entry := range items {
		if parsed, err := time.Parse(timeLayout, scalar(entry["update_time"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(timeLayout, previous); err == nil && before.After(newest) {
		return previous
	}
	return newest.UTC().Format(timeLayout)
}

func clip(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max])
}
