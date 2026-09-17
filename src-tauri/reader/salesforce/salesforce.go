// Package salesforce reads a Salesforce org behind the reader contract:
// five objects, accounts, contacts, leads, opportunities and cases, each
// flattened to text fields a mapping can point at. It uses the REST API
// over net/http alone, signs in once per process through the connected
// app's client credentials flow, and writes nothing back. Backfill walks a
// SOQL query through its query locator, and Delta is one query for the
// newest change after the cursor's mark.
package salesforce

import (
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

// APIVersion is the REST API version every path names.
const APIVersion = "v60.0"

// PageLimit is the most records one query batch returns, the largest
// batch size Salesforce honors.
const PageLimit = 2000

// minBatch is the smallest batch size Salesforce honors.
const minBatch = 200

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 200

// customFieldLimit caps the custom fields one query selects.
const customFieldLimit = 200

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

// Source is one Salesforce org reached through one connected app.
type Source struct {
	instance     string
	clientID     string
	clientSecret string
	client       *http.Client
	token        string
	custom       map[string][]customField
}

// customField is one custom field as a column: the API name the query
// selects and the column name without its `__c`.
type customField struct {
	api    string
	column contract.Column
}

// New opens a source on the credentials the panel packs: the My Domain
// URL, the consumer key and the consumer secret. A test points the
// instance at a fake.
func New(secret string) *Source {
	credentials := contract.Credentials(secret)
	return &Source{
		instance:     strings.TrimRight(credentials["instance_url"], "/"),
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		client:       &http.Client{Timeout: 60 * time.Second},
		custom:       map[string][]customField{},
	}
}

type record = map[string]any

// object is one sObject: its API name, the core fields the query selects
// and how each reads.
type object struct {
	name   string
	label  string
	sobj   string
	fields []field
}

type field struct {
	name  string
	guess string
	// select names the API field the query needs, empty for a derived column.
	select_ string
	read    func(record) string
}

var objects = []object{
	{
		name:  "accounts",
		label: "Accounts",
		sobj:  "Account",
		fields: []field{
			{"id", "id", "Id", text("Id")},
			{"name", "string", "Name", text("Name")},
			{"website", "string", "Website", text("Website")},
			{"domain", "domain", "", websiteDomain},
			{"industry", "string", "Industry", text("Industry")},
			{"type", "string", "Type", text("Type")},
			{"phone", "phone", "Phone", text("Phone")},
			{"city", "string", "BillingCity", text("BillingCity")},
			{"state", "string", "BillingState", text("BillingState")},
			{"country", "string", "BillingCountry", text("BillingCountry")},
			{"employees", "number", "NumberOfEmployees", text("NumberOfEmployees")},
			{"annual_revenue", "number", "AnnualRevenue", text("AnnualRevenue")},
			{"parent", "id", "ParentId", text("ParentId")},
			{"owner", "string", "OwnerId", text("OwnerId")},
			{"created", "date-time", "CreatedDate", when("CreatedDate")},
			{"modified", "date-time", "LastModifiedDate", when("LastModifiedDate")},
		},
	},
	{
		name:  "contacts",
		label: "Contacts",
		sobj:  "Contact",
		fields: []field{
			{"id", "id", "Id", text("Id")},
			{"first_name", "string", "FirstName", text("FirstName")},
			{"last_name", "string", "LastName", text("LastName")},
			{"name", "string", "Name", text("Name")},
			{"email", "email", "Email", text("Email")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "Phone", text("Phone")},
			{"job_title", "string", "Title", text("Title")},
			{"account", "id", "AccountId", text("AccountId")},
			{"lead_source", "string", "LeadSource", text("LeadSource")},
			{"owner", "string", "OwnerId", text("OwnerId")},
			{"created", "date-time", "CreatedDate", when("CreatedDate")},
			{"modified", "date-time", "LastModifiedDate", when("LastModifiedDate")},
		},
	},
	{
		name:  "leads",
		label: "Leads",
		sobj:  "Lead",
		fields: []field{
			{"id", "id", "Id", text("Id")},
			{"name", "string", "Name", text("Name")},
			{"first_name", "string", "FirstName", text("FirstName")},
			{"last_name", "string", "LastName", text("LastName")},
			{"company", "string", "Company", text("Company")},
			{"email", "email", "Email", text("Email")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "Phone", text("Phone")},
			{"job_title", "string", "Title", text("Title")},
			{"status", "string", "Status", text("Status")},
			{"lead_source", "string", "LeadSource", text("LeadSource")},
			{"converted", "boolean", "IsConverted", boolean("IsConverted")},
			{"converted_account", "id", "ConvertedAccountId", text("ConvertedAccountId")},
			{"converted_contact", "id", "ConvertedContactId", text("ConvertedContactId")},
			{"owner", "string", "OwnerId", text("OwnerId")},
			{"created", "date-time", "CreatedDate", when("CreatedDate")},
			{"modified", "date-time", "LastModifiedDate", when("LastModifiedDate")},
		},
	},
	{
		name:  "opportunities",
		label: "Opportunities",
		sobj:  "Opportunity",
		fields: []field{
			{"id", "id", "Id", text("Id")},
			{"name", "string", "Name", text("Name")},
			{"amount", "number", "Amount", text("Amount")},
			{"stage_name", "string", "StageName", text("StageName")},
			{"stage", "string", "IsClosed", opportunityStage},
			{"won", "boolean", "IsWon", boolean("IsWon")},
			{"probability", "number", "Probability", text("Probability")},
			{"close_date", "date", "CloseDate", text("CloseDate")},
			{"type", "string", "Type", text("Type")},
			{"lead_source", "string", "LeadSource", text("LeadSource")},
			{"account", "id", "AccountId", text("AccountId")},
			{"owner", "string", "OwnerId", text("OwnerId")},
			{"created", "date-time", "CreatedDate", when("CreatedDate")},
			{"modified", "date-time", "LastModifiedDate", when("LastModifiedDate")},
		},
	},
	{
		name:  "cases",
		label: "Cases",
		sobj:  "Case",
		fields: []field{
			{"id", "id", "Id", text("Id")},
			{"number", "string", "CaseNumber", text("CaseNumber")},
			{"subject", "string", "Subject", text("Subject")},
			{"description", "string", "Description", text("Description")},
			{"status_name", "string", "Status", text("Status")},
			{"status", "string", "IsClosed", caseStatus},
			{"priority", "string", "Priority", lowered("Priority")},
			{"origin", "string", "Origin", text("Origin")},
			{"account", "id", "AccountId", text("AccountId")},
			{"contact", "id", "ContactId", text("ContactId")},
			{"owner", "string", "OwnerId", text("OwnerId")},
			{"closed_at", "date-time", "ClosedDate", when("ClosedDate")},
			{"created", "date-time", "CreatedDate", when("CreatedDate")},
			{"modified", "date-time", "LastModifiedDate", when("LastModifiedDate")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Salesforce has no %s to read. The objects are accounts, contacts, leads, opportunities and cases.", name)}
}

// Objects signs in, which proves the connected app, and lists the five.
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

// Describe reads the first batch and answers the fields with their
// samples. The query names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	records, total, _, err := s.query(object.soql(custom, "", ""), sampleRows)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(records))
	for _, item := range records {
		rows = append(rows, object.row(item, custom))
	}
	columns := make([]contract.Column, 0, len(object.fields)+len(custom))
	for _, f := range object.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	for _, extra := range custom {
		columns = append(columns, extra.column)
	}
	return &Description{
		Source:  "salesforce",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    total,
		Bytes:   0,
		Hash:    newestModified(records, ""),
		Counted: true,
	}, nil
}

// Page reads one batch: the first through the query, the rest through the
// locator the previous answer named. Total is the query's own count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	offset := 0
	hash := ""
	locator := ""
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		locator = cursor.Token
	}
	custom := s.customFields(object)
	var records []record
	var total int
	var next string
	if locator != "" {
		records, total, next, err = s.more(locator, limit)
	} else {
		records, total, next, err = s.query(object.soql(custom, "", ""), limit)
	}
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(records))
	for _, item := range records {
		rows = append(rows, object.row(item, custom))
	}
	hash = newestModified(records, hash)
	page := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: true}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks for the one record changed after the mark, newest first.
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
	where := ""
	if mark != "" {
		where = "WHERE LastModifiedDate > " + mark
	}
	soql := fmt.Sprintf("SELECT Id, LastModifiedDate FROM %s %s ORDER BY LastModifiedDate DESC LIMIT 1", object.sobj, where)
	records, _, _, err := s.query(strings.Join(strings.Fields(soql), " "), minBatch)
	if err != nil {
		return nil, err
	}
	if len(records) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestModified(records, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// soql builds the object's query: the core fields, the custom fields, and
// a stable order so the locator walks every record once.
func (o *object) soql(custom []customField, where, order string) string {
	names := []string{}
	seen := map[string]bool{}
	for _, f := range o.fields {
		if f.select_ != "" && !seen[f.select_] {
			seen[f.select_] = true
			names = append(names, f.select_)
		}
	}
	for _, extra := range custom {
		if !seen[extra.api] {
			seen[extra.api] = true
			names = append(names, extra.api)
		}
	}
	if order == "" {
		order = "ORDER BY Id"
	}
	return strings.Join(strings.Fields(fmt.Sprintf("SELECT %s FROM %s %s %s", strings.Join(names, ", "), o.sobj, where, order)), " ")
}

// customFields reads the object's describe once per source and keeps every
// custom field as a column named without its `__c`. An org whose app
// cannot describe reads as having none, because the core columns still land.
func (s *Source) customFields(object *object) []customField {
	if cached, ok := s.custom[object.name]; ok {
		return cached
	}
	core := map[string]bool{}
	for _, f := range object.fields {
		core[f.name] = true
	}
	listed := []customField{}
	body, status, err := s.get(fmt.Sprintf("/services/data/%s/sobjects/%s/describe", APIVersion, object.sobj), nil)
	if err == nil && status == http.StatusOK {
		var described struct {
			Fields []struct {
				Name   string `json:"name"`
				Type   string `json:"type"`
				Custom bool   `json:"custom"`
			} `json:"fields"`
		}
		if json.Unmarshal(body, &described) == nil {
			for _, f := range described.Fields {
				if !f.Custom || !strings.HasSuffix(f.Name, "__c") || f.Type == "address" || f.Type == "location" || f.Type == "base64" {
					continue
				}
				name := strings.ToLower(strings.TrimSuffix(f.Name, "__c"))
				if name == "" || core[name] {
					continue
				}
				core[name] = true
				listed = append(listed, customField{api: f.Name, column: contract.Column{Name: name, Guess: guessType(f.Type)}})
			}
		}
	}
	sort.Slice(listed, func(a, b int) bool { return listed[a].column.Name < listed[b].column.Name })
	if len(listed) > customFieldLimit {
		listed = listed[:customFieldLimit]
	}
	s.custom[object.name] = listed
	return listed
}

func guessType(sfType string) string {
	switch sfType {
	case "currency", "double", "int", "percent", "long":
		return "number"
	case "boolean":
		return "boolean"
	case "date":
		return "date"
	case "datetime":
		return "date-time"
	case "email":
		return "email"
	case "phone":
		return "phone"
	default:
		return "string"
	}
}

// signIn exchanges the connected app's credentials for a session once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.instance == "" || s.clientID == "" || s.clientSecret == "" {
		return &Failure{Code: "not_connected", Message: "Connect Salesforce with its My Domain URL, consumer key and consumer secret first."}
	}
	form := url.Values{"grant_type": {"client_credentials"}, "client_id": {s.clientID}, "client_secret": {s.clientSecret}}
	request, err := http.NewRequest(http.MethodPost, s.instance+"/services/oauth2/token", strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Salesforce refused the connected app: %s. Connect Salesforce again with the consumer key and secret of an app whose client credentials flow is enabled.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Salesforce answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
		InstanceURL string `json:"instance_url"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Salesforce's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	if granted.InstanceURL != "" {
		s.instance = strings.TrimRight(granted.InstanceURL, "/")
	}
	return nil
}

// query runs one SOQL query with the batch size asked for.
func (s *Source) query(soql string, limit int) ([]record, int, string, error) {
	return s.fetch(fmt.Sprintf("/services/data/%s/query", APIVersion), url.Values{"q": {soql}}, limit)
}

// more reads the next batch through the locator path a query answered.
func (s *Source) more(locator string, limit int) ([]record, int, string, error) {
	return s.fetch(locator, nil, limit)
}

func (s *Source) fetch(path string, query url.Values, limit int) ([]record, int, string, error) {
	if limit < minBatch {
		limit = minBatch
	}
	if limit > PageLimit {
		limit = PageLimit
	}
	target := path
	if !strings.HasPrefix(target, "http") {
		target = s.instance + target
	}
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, "", &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Sforce-Query-Options", "batchSize="+strconv.Itoa(limit))
	body, status, err := s.do(request)
	if err != nil {
		return nil, 0, "", err
	}
	if err := refusal(status, body); err != nil {
		return nil, 0, "", err
	}
	var answer struct {
		TotalSize      int      `json:"totalSize"`
		Done           bool     `json:"done"`
		NextRecordsURL string   `json:"nextRecordsUrl"`
		Records        []record `json:"records"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, 0, "", &Failure{Code: "source", Message: fmt.Sprintf("Salesforce's answer does not parse: %s.", err)}
	}
	next := ""
	if !answer.Done && answer.NextRecordsURL != "" {
		next = answer.NextRecordsURL
	}
	return answer.Records, answer.TotalSize, next, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	target := s.instance + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Salesforce did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Salesforce's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Salesforce answers into the one failure the
// runtime reads. Salesforce answers a spent daily API budget as a 403 with
// the code REQUEST_LIMIT_EXCEEDED, so the code decides, not the status.
func refusal(status int, body []byte) error {
	code, message := errorCode(body)
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Salesforce refused the session. Connect Salesforce again."}
	case code == "REQUEST_LIMIT_EXCEEDED":
		return &Failure{Code: "rate_limited", Message: "Salesforce's daily API request budget is spent. Run again tomorrow."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Salesforce refused the request: %s. The connected app's run-as user needs read access to the object.", message)}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Salesforce answered %d: %s", status, message)}
	}
	return nil
}

func errorCode(body []byte) (string, string) {
	var failures []struct {
		ErrorCode string `json:"errorCode"`
		Message   string `json:"message"`
	}
	if json.Unmarshal(body, &failures) == nil && len(failures) > 0 {
		return failures[0].ErrorCode, failures[0].Message
	}
	return "", contract.Clip(strings.TrimSpace(string(body)), 200)
}

func oauthMessage(body []byte) string {
	var failure struct {
		Error       string `json:"error"`
		Description string `json:"error_description"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error != "" {
		if failure.Description != "" {
			return failure.Error + ", " + failure.Description
		}
		return failure.Error
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record: the core fields, then every custom field as its
// own column.
func (o *object) row(item record, custom []customField) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	for _, extra := range custom {
		value := contract.Scalar(item[extra.api])
		if extra.column.Guess == "date-time" {
			value = formatTime(value)
		}
		row[extra.column.Name] = value
	}
	return row
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

func lowered(key string) func(record) string {
	return func(item record) string { return strings.ToLower(contract.Scalar(item[key])) }
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

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Salesforce's `2023-11-14T22:13:20.000+0000` as RFC 3339
// in UTC.
func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	for _, layout := range []string{"2006-01-02T15:04:05.000-0700", time.RFC3339Nano, "2006-01-02"} {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed.UTC().Format(time.RFC3339)
		}
	}
	return value
}

func emailDomain(item record) string {
	email := contract.Scalar(item["Email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// websiteDomain reads a website as its registrable host, without scheme,
// `www.` or path.
func websiteDomain(item record) string {
	site := strings.TrimSpace(contract.Scalar(item["Website"]))
	if site == "" {
		return ""
	}
	if !strings.Contains(site, "://") {
		site = "https://" + site
	}
	parsed, err := url.Parse(site)
	if err != nil || parsed.Host == "" {
		return ""
	}
	host := strings.ToLower(parsed.Hostname())
	return strings.TrimPrefix(host, "www.")
}

// opportunityStage folds a Salesforce stage onto the deal kind's stages:
// the default stage names by name, and any other by whether it is closed
// and won. A stage the reader cannot place reads as empty, and the mapping
// keeps the stage name beside it.
func opportunityStage(item record) string {
	switch strings.ToLower(contract.Scalar(item["StageName"])) {
	case "prospecting":
		return "discovery"
	case "qualification":
		return "qualification"
	case "needs analysis", "value proposition", "id. decision makers", "perception analysis", "proposal/price quote":
		return "proposal"
	case "negotiation/review":
		return "negotiation"
	case "closed won":
		return "won"
	case "closed lost":
		return "lost"
	}
	if won, _ := item["IsWon"].(bool); won {
		return "won"
	}
	if closed, _ := item["IsClosed"].(bool); closed {
		return "lost"
	}
	return ""
}

// caseStatus folds the default support statuses onto the ticket kind's
// statuses, and any other by whether the case is closed.
func caseStatus(item record) string {
	switch strings.ToLower(contract.Scalar(item["Status"])) {
	case "new", "working", "escalated":
		return "open"
	case "on hold", "waiting", "pending":
		return "pending"
	case "closed":
		return "closed"
	}
	if closed, _ := item["IsClosed"].(bool); closed {
		return "closed"
	}
	return "open"
}

// newestModified is the change mark: the latest LastModifiedDate in a
// list, as Salesforce writes it so a query reads it back, or the previous
// mark when nothing newer appears.
func newestModified(records []record, previous string) string {
	var newest time.Time
	for _, item := range records {
		if parsed, ok := parseTime(contract.Scalar(item["LastModifiedDate"])); ok && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, ok := parseTime(previous); ok && before.After(newest) {
		return previous
	}
	return newest.UTC().Format("2006-01-02T15:04:05.000Z")
}

func parseTime(value string) (time.Time, bool) {
	for _, layout := range []string{"2006-01-02T15:04:05.000-0700", time.RFC3339Nano} {
		if parsed, err := time.Parse(layout, strings.TrimSpace(value)); err == nil {
			return parsed, true
		}
	}
	return time.Time{}, false
}
