// Package hubspot reads a HubSpot portal behind the reader contract: four
// objects, contacts, companies, deals and tickets, each flattened to text
// fields a mapping can point at. It uses the CRM v3 REST API over net/http
// alone, with a private app access token as the bearer, and it writes
// nothing back. Backfill walks the paged list endpoints on the general
// budget. Delta alone spends the separate search budget, one search per
// object per call, so the scarce cap never pays for a page.
package hubspot

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is HubSpot's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.hubapi.com"

// PageLimit is the most records one HubSpot list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// customPropertyLimit caps the custom properties one list call asks for, so
// the request line stays inside what HubSpot accepts.
const customPropertyLimit = 200

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

// Source is one HubSpot portal.
type Source struct {
	token   string
	baseURL string
	client  *http.Client
	custom  map[string][]property
}

// New opens a source on a private app access token. An empty base URL is
// the live API.
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

// record is one HubSpot object as the API answers it: an id, a properties
// map, timestamps and the associations the list call was asked for.
type record = map[string]any

// object is one HubSpot object type: its list path, the property that marks
// its last change, the associations a row carries and the core fields in
// the order Describe lists them.
type object struct {
	name         string
	label        string
	path         string
	modified     string
	associations []string
	fields       []field
}

// field names one text column, how the kind schema should type it, the
// HubSpot property it needs from the list call, and how it reads.
type field struct {
	name     string
	guess    string
	property string
	read     func(record) string
}

// property is one custom property the portal defines, as a column.
type property struct {
	name  string
	guess string
}

var objects = []object{
	{
		name:         "contacts",
		label:        "Contacts",
		path:         "/crm/v3/objects/contacts",
		modified:     "lastmodifieddate",
		associations: []string{"companies"},
		fields: []field{
			{"id", "id", "", text("id")},
			{"first_name", "string", "firstname", prop("firstname")},
			{"last_name", "string", "lastname", prop("lastname")},
			{"name", "string", "", contactName},
			{"email", "email", "email", prop("email")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "phone", prop("phone")},
			{"job_title", "string", "jobtitle", prop("jobtitle")},
			{"company_name", "string", "company", prop("company")},
			{"company", "id", "", firstAssociation("companies")},
			{"lifecycle_stage", "string", "lifecyclestage", prop("lifecyclestage")},
			{"lead_status", "string", "hs_lead_status", prop("hs_lead_status")},
			{"owner", "string", "hubspot_owner_id", prop("hubspot_owner_id")},
			{"created", "date-time", "createdate", propTime("createdate")},
			{"modified", "date-time", "lastmodifieddate", propTime("lastmodifieddate")},
			{"archived", "boolean", "", boolean("archived")},
		},
	},
	{
		name:         "companies",
		label:        "Companies",
		path:         "/crm/v3/objects/companies",
		modified:     "hs_lastmodifieddate",
		associations: nil,
		fields: []field{
			{"id", "id", "", text("id")},
			{"name", "string", "name", prop("name")},
			{"domain", "domain", "domain", prop("domain")},
			{"website", "string", "website", prop("website")},
			{"industry", "string", "industry", prop("industry")},
			{"phone", "phone", "phone", prop("phone")},
			{"city", "string", "city", prop("city")},
			{"state", "string", "state", prop("state")},
			{"country", "string", "country", prop("country")},
			{"employees", "number", "numberofemployees", prop("numberofemployees")},
			{"annual_revenue", "number", "annualrevenue", prop("annualrevenue")},
			{"lifecycle_stage", "string", "lifecyclestage", prop("lifecyclestage")},
			{"owner", "string", "hubspot_owner_id", prop("hubspot_owner_id")},
			{"created", "date-time", "createdate", propTime("createdate")},
			{"modified", "date-time", "hs_lastmodifieddate", propTime("hs_lastmodifieddate")},
			{"archived", "boolean", "", boolean("archived")},
		},
	},
	{
		name:         "deals",
		label:        "Deals",
		path:         "/crm/v3/objects/deals",
		modified:     "hs_lastmodifieddate",
		associations: []string{"companies", "contacts"},
		fields: []field{
			{"id", "id", "", text("id")},
			{"name", "string", "dealname", prop("dealname")},
			{"amount", "number", "amount", prop("amount")},
			{"currency", "string", "deal_currency_code", prop("deal_currency_code")},
			{"pipeline", "string", "pipeline", prop("pipeline")},
			{"stage_id", "string", "dealstage", prop("dealstage")},
			{"stage", "string", "hs_is_closed_won", dealStage},
			{"closed", "boolean", "hs_is_closed", propBoolean("hs_is_closed")},
			{"close_date", "date-time", "closedate", propTime("closedate")},
			{"company", "id", "", firstAssociation("companies")},
			{"contacts", "string", "", associations("contacts")},
			{"owner", "string", "hubspot_owner_id", prop("hubspot_owner_id")},
			{"created", "date-time", "createdate", propTime("createdate")},
			{"modified", "date-time", "hs_lastmodifieddate", propTime("hs_lastmodifieddate")},
			{"archived", "boolean", "", boolean("archived")},
		},
	},
	{
		name:         "tickets",
		label:        "Tickets",
		path:         "/crm/v3/objects/tickets",
		modified:     "hs_lastmodifieddate",
		associations: []string{"companies", "contacts"},
		fields: []field{
			{"id", "id", "", text("id")},
			{"subject", "string", "subject", prop("subject")},
			{"content", "string", "content", prop("content")},
			{"pipeline", "string", "hs_pipeline", prop("hs_pipeline")},
			{"stage_id", "string", "hs_pipeline_stage", prop("hs_pipeline_stage")},
			{"status", "string", "", ticketStatus},
			{"priority", "string", "hs_ticket_priority", lowered("hs_ticket_priority")},
			{"company", "id", "", firstAssociation("companies")},
			{"contacts", "string", "", associations("contacts")},
			{"owner", "string", "hubspot_owner_id", prop("hubspot_owner_id")},
			{"created", "date-time", "createdate", propTime("createdate")},
			{"modified", "date-time", "hs_lastmodifieddate", propTime("hs_lastmodifieddate")},
			{"archived", "boolean", "", boolean("archived")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("HubSpot has no %s to read. The objects are contacts, companies, deals and tickets.", name)}
}

// Objects lists the four objects the reader knows.
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
// HubSpot's list carries no count, so Rows is the rows read and Counted is
// false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	custom := s.customProperties(object)
	items, _, err := s.list(object.path, s.listQuery(object, custom, sampleRows, ""))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	columns := make([]property, 0, len(object.fields)+len(custom))
	for _, f := range object.fields {
		columns = append(columns, property{f.name, f.guess})
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
		Source:  "hubspot",
		Object:  name,
		Label:   object.label,
		Fields:  fields,
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestModified(items, ""),
		Counted: false,
	}, nil
}

// Page reads one list page after the cursor's token. Total is the rows seen
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
	after := ""
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		after = cursor.Token
	}
	custom := s.customProperties(object)
	items, next, err := s.list(object.path, s.listQuery(object, custom, limit, after))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	hash = newestModified(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks the search endpoint for one record changed after the cursor's
// mark, newest first. That is the one search per object per cycle the
// budget allows, and the mark it answers is the newest change it saw.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	body := map[string]any{
		"limit":      1,
		"properties": []string{object.modified},
		"sorts":      []map[string]string{{"propertyName": object.modified, "direction": "DESCENDING"}},
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	if mark != "" {
		body["filterGroups"] = []map[string]any{{"filters": []map[string]string{
			{"propertyName": object.modified, "operator": "GT", "value": mark},
		}}}
	}
	total, items, err := s.search(object.path+"/search", body)
	if err != nil {
		return nil, err
	}
	if total == 0 || len(items) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestModified(items, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// listQuery builds the list call: the properties every core field needs,
// the custom properties as their own columns, the associations a row
// carries, and the page token.
func (s *Source) listQuery(object *object, custom []property, limit int, after string) url.Values {
	names := []string{}
	seen := map[string]bool{}
	for _, f := range object.fields {
		if f.property != "" && !seen[f.property] {
			seen[f.property] = true
			names = append(names, f.property)
		}
	}
	for _, extra := range []string{object.modified, "hs_is_closed", "hs_is_closed_won"} {
		if (extra == object.modified || object.name == "deals") && !seen[extra] {
			seen[extra] = true
			names = append(names, extra)
		}
	}
	for _, column := range custom {
		if !seen[column.name] {
			seen[column.name] = true
			names = append(names, column.name)
		}
	}
	query := url.Values{
		"limit":      {strconv.Itoa(limit)},
		"archived":   {"false"},
		"properties": {strings.Join(names, ",")},
	}
	if len(object.associations) > 0 {
		query.Set("associations", strings.Join(object.associations, ","))
	}
	if after != "" {
		query.Set("after", after)
	}
	return query
}

// customProperties reads the portal's own properties on an object once per
// source. A portal whose token lacks the schema scope reads as having none,
// because the core columns still land.
func (s *Source) customProperties(object *object) []property {
	if cached, ok := s.custom[object.name]; ok {
		return cached
	}
	core := map[string]bool{}
	for _, f := range object.fields {
		core[f.name] = true
		if f.property != "" {
			core[f.property] = true
		}
	}
	listed := []property{}
	body, status, err := s.get("/crm/v3/properties/"+object.name, url.Values{})
	if err == nil && status == http.StatusOK {
		var answer struct {
			Results []struct {
				Name           string `json:"name"`
				Type           string `json:"type"`
				HubspotDefined bool   `json:"hubspotDefined"`
				Hidden         bool   `json:"hidden"`
			} `json:"results"`
		}
		if json.Unmarshal(body, &answer) == nil {
			for _, result := range answer.Results {
				if result.HubspotDefined || result.Hidden || result.Name == "" || core[result.Name] {
					continue
				}
				listed = append(listed, property{result.Name, guessType(result.Type)})
			}
		}
	}
	sort.Slice(listed, func(a, b int) bool { return listed[a].name < listed[b].name })
	if len(listed) > customPropertyLimit {
		listed = listed[:customPropertyLimit]
	}
	s.custom[object.name] = listed
	return listed
}

func guessType(hubspotType string) string {
	switch hubspotType {
	case "number":
		return "number"
	case "bool":
		return "boolean"
	case "date":
		return "date"
	case "datetime":
		return "date-time"
	default:
		return "string"
	}
}

// list fetches one page of a list endpoint and answers its results and the
// next page token.
func (s *Source) list(path string, query url.Values) ([]record, string, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(path, status, body); err != nil {
		return nil, "", err
	}
	var listed struct {
		Results []record `json:"results"`
		Paging  struct {
			Next struct {
				After string `json:"after"`
			} `json:"next"`
		} `json:"paging"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("HubSpot's answer does not parse: %s.", err)}
	}
	return listed.Results, listed.Paging.Next.After, nil
}

// search posts one search and answers its total and results.
func (s *Source) search(path string, body map[string]any) (int, []record, error) {
	encoded, err := json.Marshal(body)
	if err != nil {
		return 0, nil, &Failure{Code: "source", Message: err.Error()}
	}
	request, err := http.NewRequest(http.MethodPost, s.baseURL+path, bytes.NewReader(encoded))
	if err != nil {
		return 0, nil, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/json")
	answer, status, err := s.do(request)
	if err != nil {
		return 0, nil, err
	}
	if err := s.refusal(path, status, answer); err != nil {
		return 0, nil, err
	}
	var found struct {
		Total   int      `json:"total"`
		Results []record `json:"results"`
	}
	if err := json.Unmarshal(answer, &found); err != nil {
		return 0, nil, &Failure{Code: "source", Message: fmt.Sprintf("HubSpot's answer does not parse: %s.", err)}
	}
	return found.Total, found.Results, nil
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
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("HubSpot did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("HubSpot's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status HubSpot answers into the one failure the runtime
// reads. A 429 branches on the policy in the body, never on the status
// alone: a secondly limit waits a minute, a daily limit waits a day.
func (s *Source) refusal(path string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "HubSpot refused the access token. Connect HubSpot again with a private app token that works."}
	case status == http.StatusForbidden:
		object := objectOf(path)
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("HubSpot refused the token for %s. The private app needs the crm.objects.%s.read scope.", object, object)}
	case status == http.StatusTooManyRequests:
		if strings.EqualFold(policyName(body), "DAILY") {
			return &Failure{Code: "rate_limited", Message: "HubSpot's daily request budget is spent. Run again tomorrow."}
		}
		return &Failure{Code: "rate_limited", Message: "HubSpot asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("HubSpot answered %d: %s", status, hubspotMessage(body))}
	}
	return nil
}

func objectOf(path string) string {
	parts := strings.Split(strings.Trim(path, "/"), "/")
	for index, part := range parts {
		if part == "objects" && index+1 < len(parts) {
			return parts[index+1]
		}
	}
	return "the object"
}

func policyName(body []byte) string {
	var failure struct {
		PolicyName string `json:"policyName"`
	}
	if json.Unmarshal(body, &failure) == nil {
		return failure.PolicyName
	}
	return ""
}

func hubspotMessage(body []byte) string {
	var failure struct {
		Message string `json:"message"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Message != "" {
		return failure.Message
	}
	return clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record: the core fields, then every custom property as
// its own column, so a mapping can point at them.
func (o *object) row(item record, custom []property) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	for _, column := range custom {
		value := prop(column.name)(item)
		if column.guess == "date-time" || column.guess == "date" {
			value = formatTime(value)
		}
		row[column.name] = value
	}
	return row
}

func properties(item record) map[string]any {
	if props, ok := item["properties"].(map[string]any); ok {
		return props
	}
	return map[string]any{}
}

func text(key string) func(record) string {
	return func(item record) string { return scalar(item[key]) }
}

func prop(name string) func(record) string {
	return func(item record) string { return scalar(properties(item)[name]) }
}

func lowered(name string) func(record) string {
	return func(item record) string { return strings.ToLower(scalar(properties(item)[name])) }
}

func propTime(name string) func(record) string {
	return func(item record) string { return formatTime(scalar(properties(item)[name])) }
}

func propBoolean(name string) func(record) string {
	return func(item record) string {
		switch strings.ToLower(scalar(properties(item)[name])) {
		case "true":
			return "yes"
		case "false":
			return "no"
		default:
			return ""
		}
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

// formatTime reads an ISO timestamp or a millisecond count as RFC 3339 in
// UTC, the shape every other reader writes.
func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	if parsed, err := time.Parse("2006-01-02", value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	if millis, err := strconv.ParseInt(value, 10, 64); err == nil && millis > 0 {
		return time.UnixMilli(millis).UTC().Format(time.RFC3339)
	}
	return value
}

func contactName(item record) string {
	props := properties(item)
	name := strings.TrimSpace(strings.TrimSpace(scalar(props["firstname"])) + " " + strings.TrimSpace(scalar(props["lastname"])))
	if name != "" {
		return name
	}
	return scalar(props["email"])
}

func emailDomain(item record) string {
	email := scalar(properties(item)["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

func associationIDs(item record, kind string) []string {
	associated, ok := item["associations"].(map[string]any)
	if !ok {
		return nil
	}
	group, ok := associated[kind].(map[string]any)
	if !ok {
		return nil
	}
	results, ok := group["results"].([]any)
	if !ok {
		return nil
	}
	ids := []string{}
	seen := map[string]bool{}
	for _, entry := range results {
		result, _ := entry.(map[string]any)
		id := scalar(result["id"])
		if id != "" && !seen[id] {
			seen[id] = true
			ids = append(ids, id)
		}
	}
	return ids
}

func firstAssociation(kind string) func(record) string {
	return func(item record) string {
		ids := associationIDs(item, kind)
		if len(ids) == 0 {
			return ""
		}
		return ids[0]
	}
}

func associations(kind string) func(record) string {
	return func(item record) string { return strings.Join(associationIDs(item, kind), ",") }
}

// dealStage folds a HubSpot deal onto the deal kind's stages: the default
// pipeline's stage ids by name, and any other pipeline by whether the deal
// is closed and won. A stage the reader cannot place reads as empty, and
// the mapping keeps the source stage id beside it.
func dealStage(item record) string {
	props := properties(item)
	switch strings.ToLower(scalar(props["dealstage"])) {
	case "appointmentscheduled":
		return "discovery"
	case "qualifiedtobuy":
		return "qualification"
	case "presentationscheduled", "decisionmakerboughtin":
		return "proposal"
	case "contractsent":
		return "negotiation"
	case "closedwon":
		return "won"
	case "closedlost":
		return "lost"
	}
	if strings.EqualFold(scalar(props["hs_is_closed_won"]), "true") {
		return "won"
	}
	if strings.EqualFold(scalar(props["hs_is_closed"]), "true") {
		return "lost"
	}
	return ""
}

// ticketStatus folds the default support pipeline's stage ids onto the
// ticket kind's statuses. Another pipeline's stage reads as empty.
func ticketStatus(item record) string {
	switch scalar(properties(item)["hs_pipeline_stage"]) {
	case "1", "3":
		return "open"
	case "2":
		return "pending"
	case "4":
		return "closed"
	default:
		return ""
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

// modifiedMillis reads a record's last change as milliseconds: the
// modified property when the list asked for it, else the record's updatedAt.
func modifiedMillis(item record) int64 {
	props := properties(item)
	for _, key := range []string{"hs_lastmodifieddate", "lastmodifieddate"} {
		if millis := parseMillis(scalar(props[key])); millis > 0 {
			return millis
		}
	}
	return parseMillis(scalar(item["updatedAt"]))
}

func parseMillis(value string) int64 {
	value = strings.TrimSpace(value)
	if value == "" {
		return 0
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UnixMilli()
	}
	if millis, err := strconv.ParseInt(value, 10, 64); err == nil {
		return millis
	}
	return 0
}

// newestModified is the change mark: the latest change in a list as
// milliseconds in text, or the previous mark when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest int64
	for _, item := range items {
		if millis := modifiedMillis(item); millis > newest {
			newest = millis
		}
	}
	if newest == 0 {
		return previous
	}
	if before, err := strconv.ParseInt(previous, 10, 64); err == nil && before > newest {
		return previous
	}
	return strconv.FormatInt(newest, 10)
}

func clip(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max])
}
