// Package zoho reads a Zoho CRM org behind the reader contract: four
// modules, leads, contacts, accounts and deals, each flattened to text
// fields a mapping can point at. It uses the v2 REST API over net/http
// alone, trades the refresh token for an access token once per process,
// and writes nothing back. Backfill walks the paged module lists, and
// Delta is one request with If-Modified-Since at the cursor's mark, which
// Zoho answers 304 when nothing changed.
package zoho

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

// DefaultAccountsURL is the accounts host of a US data center. A customer
// in another data center pastes its own, such as accounts.zoho.eu.
const DefaultAccountsURL = "https://accounts.zoho.com"

// DefaultBaseURL is the API host of a US data center. The token answer
// names the customer's own, and a test points the reader at a fake.
const DefaultBaseURL = "https://www.zohoapis.com"

// PageLimit is the most records one module list call returns.
const PageLimit = 200

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 200

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

// Source is one Zoho CRM org reached through one self client.
type Source struct {
	accounts     string
	baseURL      string
	pinned       bool
	clientID     string
	clientSecret string
	refreshToken string
	client       *http.Client
	token        string
	custom       map[string][]customField
}

// customField is one custom field as a column: the API name the record
// carries it under and the column name the org gave it.
type customField struct {
	api    string
	column contract.Column
}

// New opens a source on the credentials the panel packs: the client id,
// the client secret, the refresh token and the accounts domain. An empty
// base URL is the API host the token answer names, and a base URL pins
// both the accounts host and the API host, so a test reaches one fake.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	accounts := strings.TrimRight(strings.TrimSpace(credentials["accounts_domain"]), "/")
	if accounts != "" && !strings.Contains(accounts, "://") {
		accounts = "https://" + accounts
	}
	if accounts == "" {
		accounts = DefaultAccountsURL
	}
	source := &Source{
		accounts:     accounts,
		baseURL:      apiHost(accounts),
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		refreshToken: credentials["refresh_token"],
		client:       &http.Client{Timeout: 30 * time.Second},
		custom:       map[string][]customField{},
	}
	if baseURL != "" {
		source.accounts = strings.TrimRight(baseURL, "/")
		source.baseURL = source.accounts
		source.pinned = true
	}
	return source
}

// apiHost derives the API host from the accounts host, so a customer in
// the EU data center reaches www.zohoapis.eu before the token answer
// names it.
func apiHost(accounts string) string {
	parsed, err := url.Parse(accounts)
	if err != nil || parsed.Host == "" {
		return DefaultBaseURL
	}
	host := parsed.Hostname()
	if strings.HasPrefix(host, "accounts.zoho.") {
		return "https://www.zohoapis." + strings.TrimPrefix(host, "accounts.zoho.")
	}
	return DefaultBaseURL
}

type record = map[string]any

// object is one Zoho module: its API name and the core fields in the
// order Describe lists them.
type object struct {
	name   string
	label  string
	module string
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
		name:   "leads",
		label:  "Leads",
		module: "Leads",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("Full_Name")},
			{"first_name", "string", text("First_Name")},
			{"last_name", "string", text("Last_Name")},
			{"company", "string", text("Company")},
			{"email", "email", text("Email")},
			{"email_domain", "domain", emailDomain("Email")},
			{"phone", "phone", text("Phone")},
			{"mobile", "phone", text("Mobile")},
			{"job_title", "string", text("Designation")},
			{"status", "string", text("Lead_Status")},
			{"lead_source", "string", text("Lead_Source")},
			{"industry", "string", text("Industry")},
			{"website", "string", text("Website")},
			{"city", "string", text("City")},
			{"state", "string", text("State")},
			{"country", "string", text("Country")},
			{"owner", "string", refName("Owner")},
			{"owner_id", "id", refID("Owner")},
			{"created", "date-time", when("Created_Time")},
			{"modified", "date-time", when("Modified_Time")},
		},
	},
	{
		name:   "contacts",
		label:  "Contacts",
		module: "Contacts",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("Full_Name")},
			{"first_name", "string", text("First_Name")},
			{"last_name", "string", text("Last_Name")},
			{"email", "email", text("Email")},
			{"email_domain", "domain", emailDomain("Email")},
			{"phone", "phone", text("Phone")},
			{"mobile", "phone", text("Mobile")},
			{"job_title", "string", text("Title")},
			{"department", "string", text("Department")},
			{"account_id", "id", refID("Account_Name")},
			{"account_name", "string", refName("Account_Name")},
			{"lead_source", "string", text("Lead_Source")},
			{"city", "string", text("Mailing_City")},
			{"country", "string", text("Mailing_Country")},
			{"owner", "string", refName("Owner")},
			{"owner_id", "id", refID("Owner")},
			{"created", "date-time", when("Created_Time")},
			{"modified", "date-time", when("Modified_Time")},
		},
	},
	{
		name:   "accounts",
		label:  "Accounts",
		module: "Accounts",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("Account_Name")},
			{"website", "string", text("Website")},
			{"domain", "domain", websiteDomain("Website")},
			{"industry", "string", text("Industry")},
			{"type", "string", text("Account_Type")},
			{"phone", "phone", text("Phone")},
			{"city", "string", text("Billing_City")},
			{"state", "string", text("Billing_State")},
			{"country", "string", text("Billing_Country")},
			{"employees", "number", text("Employees")},
			{"annual_revenue", "number", text("Annual_Revenue")},
			{"parent_id", "id", refID("Parent_Account")},
			{"owner", "string", refName("Owner")},
			{"owner_id", "id", refID("Owner")},
			{"created", "date-time", when("Created_Time")},
			{"modified", "date-time", when("Modified_Time")},
		},
	},
	{
		name:   "deals",
		label:  "Deals",
		module: "Deals",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("Deal_Name")},
			{"amount", "number", text("Amount")},
			{"currency", "string", text("Currency")},
			{"stage_name", "string", text("Stage")},
			{"stage", "string", dealStage},
			{"probability", "number", text("Probability")},
			{"close_date", "date", text("Closing_Date")},
			{"type", "string", text("Type")},
			{"lead_source", "string", text("Lead_Source")},
			{"pipeline", "string", text("Pipeline")},
			{"account_id", "id", refID("Account_Name")},
			{"account_name", "string", refName("Account_Name")},
			{"contact_id", "id", refID("Contact_Name")},
			{"owner", "string", refName("Owner")},
			{"owner_id", "id", refID("Owner")},
			{"created", "date-time", when("Created_Time")},
			{"modified", "date-time", when("Modified_Time")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Zoho CRM has no %s to read. The objects are leads, contacts, accounts and deals.", name)}
}

// Objects trades the refresh token, which proves the client, and lists
// the four modules.
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
// samples. Zoho's list names no total, so Rows is the rows read and
// Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	custom := s.customFields(object)
	items, _, err := s.list(object, url.Values{"page": {"1"}, "per_page": {strconv.Itoa(sampleRows)}}, "")
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
	return &Description{
		Source:  "zoho",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestModified(items, ""),
		Counted: false,
	}, nil
}

// Page reads one page of the module from the cursor's page number. Total
// is the rows seen so far, because the list names no count.
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
	custom := s.customFields(object)
	items, more, err := s.list(object, url.Values{"page": {strconv.Itoa(page)}, "per_page": {strconv.Itoa(limit)}}, "")
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item, custom))
	}
	hash = newestModified(items, hash)
	answer := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if more {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(page + 1)}
	}
	return answer, nil
}

// Delta asks for the one record changed after the mark, newest first,
// through If-Modified-Since. Zoho answers 304 when nothing changed.
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
	query := url.Values{"page": {"1"}, "per_page": {"1"}, "sort_by": {"Modified_Time"}, "sort_order": {"desc"}}
	items, _, err := s.list(object, query, mark)
	if err != nil {
		return nil, err
	}
	if len(items) == 0 {
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

// signIn trades the refresh token for an access token once per process.
// Zoho answers a refused client with status 200 and an error field, so
// the body decides, not the status alone.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" || s.refreshToken == "" {
		return &Failure{Code: "not_connected", Message: "Connect Zoho CRM with its client id, client secret, refresh token and accounts domain first."}
	}
	form := url.Values{
		"grant_type":    {"refresh_token"},
		"client_id":     {s.clientID},
		"client_secret": {s.clientSecret},
		"refresh_token": {s.refreshToken},
	}
	request, err := http.NewRequest(http.MethodPost, s.accounts+"/oauth/v2/token", strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	var granted struct {
		AccessToken string `json:"access_token"`
		APIDomain   string `json:"api_domain"`
		Error       string `json:"error"`
	}
	_ = json.Unmarshal(body, &granted)
	switch {
	case status == http.StatusUnauthorized || status == http.StatusBadRequest || granted.Error != "":
		message := granted.Error
		if message == "" {
			message = contract.Clip(strings.TrimSpace(string(body)), 200)
		}
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Zoho CRM refused the client: %s. Connect Zoho CRM again with the client id, client secret and refresh token of a self client on the same accounts domain.", message)}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Zoho CRM answered %d at sign-in: %s", status, contract.Clip(strings.TrimSpace(string(body)), 200))}
	case granted.AccessToken == "":
		return &Failure{Code: "source", Message: "Zoho CRM's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	if granted.APIDomain != "" && !s.pinned {
		s.baseURL = strings.TrimRight(granted.APIDomain, "/")
	}
	return nil
}

// customFields reads the module's field settings once per source and
// keeps every custom field as a column named as the org named it. An org
// whose client lacks the settings scope reads as having none, because the
// core columns still land.
func (s *Source) customFields(object *object) []customField {
	if cached, ok := s.custom[object.name]; ok {
		return cached
	}
	taken := map[string]bool{}
	for _, f := range object.fields {
		taken[f.name] = true
	}
	listed := []customField{}
	body, status, err := s.get("/crm/v2/settings/fields", url.Values{"module": {object.module}}, "")
	if err == nil && status == http.StatusOK {
		var answer struct {
			Fields []struct {
				APIName    string `json:"api_name"`
				FieldLabel string `json:"field_label"`
				DataType   string `json:"data_type"`
				Custom     bool   `json:"custom_field"`
			} `json:"fields"`
		}
		if json.Unmarshal(body, &answer) == nil {
			for _, f := range answer.Fields {
				if !f.Custom || f.APIName == "" || f.DataType == "subform" || f.DataType == "fileupload" || f.DataType == "imageupload" {
					continue
				}
				name := columnName(f.FieldLabel)
				if name == "" {
					name = columnName(f.APIName)
				}
				if name == "" || taken[name] {
					continue
				}
				taken[name] = true
				listed = append(listed, customField{api: f.APIName, column: contract.Column{Name: name, Guess: guessType(f.DataType)}})
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

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a field's shown label as one snake_case column.
func columnName(label string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(label), "_"), "_")
}

func guessType(dataType string) string {
	switch dataType {
	case "integer", "double", "currency", "bigint", "percent", "autonumber":
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

// list fetches one page of a module and answers its records and whether
// more follow. A 304 is an empty page, the answer Zoho gives when nothing
// changed after the If-Modified-Since mark or the module holds no record.
func (s *Source) list(object *object, query url.Values, since string) ([]record, bool, error) {
	body, status, err := s.get("/crm/v2/"+object.module, query, since)
	if err != nil {
		return nil, false, err
	}
	if status == http.StatusNoContent || status == http.StatusNotModified {
		return []record{}, false, nil
	}
	if err := refusal(status, body); err != nil {
		return nil, false, err
	}
	var listed struct {
		Data []record `json:"data"`
		Info struct {
			MoreRecords bool `json:"more_records"`
		} `json:"info"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, false, &Failure{Code: "source", Message: fmt.Sprintf("Zoho CRM's answer does not parse: %s.", err)}
	}
	return listed.Data, listed.Info.MoreRecords, nil
}

func (s *Source) get(path string, query url.Values, since string) ([]byte, int, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Zoho-oauthtoken "+s.token)
	if since != "" {
		request.Header.Set("If-Modified-Since", since)
	}
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Zoho CRM did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Zoho CRM's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Zoho answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	code, message := zohoError(body)
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Zoho CRM refused the access token. Connect Zoho CRM again."}
	case status == http.StatusForbidden || code == "NO_PERMISSION" || code == "OAUTH_SCOPE_MISMATCH":
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Zoho CRM refused the request: %s. The self client needs the ZohoCRM.modules.READ and ZohoCRM.settings.fields.READ scopes.", message)}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Zoho CRM asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Zoho CRM answered %d: %s", status, message)}
	}
	return nil
}

func zohoError(body []byte) (string, string) {
	var failure struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Code != "" {
		if failure.Message != "" {
			return failure.Code, failure.Code + ", " + failure.Message
		}
		return failure.Code, failure.Code
	}
	return "", contract.Clip(strings.TrimSpace(string(body)), 200)
}

// row flattens one record: the core fields, then every custom field as
// its own column.
func (o *object) row(item record, custom []customField) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	for _, extra := range custom {
		value := customValue(item[extra.api])
		if extra.column.Guess == "date-time" {
			value = formatTime(value)
		}
		row[extra.column.Name] = value
	}
	return row
}

// customValue reads a custom field's value: a scalar as text, a lookup by
// its name, a multi-select joined.
func customValue(value any) string {
	switch typed := value.(type) {
	case map[string]any:
		if shown := contract.Scalar(typed["name"]); shown != "" {
			return shown
		}
		return contract.Scalar(typed["id"])
	case []any:
		return contract.Joined(typed, true)
	default:
		return contract.Scalar(value)
	}
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

// refID reads a lookup's id, and refName its shown name.
func refID(key string) func(record) string {
	return func(item record) string {
		if lookup, ok := item[key].(map[string]any); ok {
			return contract.Scalar(lookup["id"])
		}
		return ""
	}
}

func refName(key string) func(record) string {
	return func(item record) string {
		if lookup, ok := item[key].(map[string]any); ok {
			return contract.Scalar(lookup["name"])
		}
		return contract.Scalar(item[key])
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Zoho's offset timestamp as RFC 3339 in UTC.
func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	for _, layout := range []string{time.RFC3339Nano, "2006-01-02"} {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed.UTC().Format(time.RFC3339)
		}
	}
	return value
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

// websiteDomain reads a website as its host, without scheme, `www.` or
// path.
func websiteDomain(key string) func(record) string {
	return func(item record) string {
		site := strings.TrimSpace(contract.Scalar(item[key]))
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
		return strings.TrimPrefix(strings.ToLower(parsed.Hostname()), "www.")
	}
}

// dealStage folds a Zoho stage onto the deal kind's stages by the default
// stage names. A stage the reader cannot place reads as empty, and the
// mapping keeps the stage name beside it.
func dealStage(item record) string {
	switch strings.ToLower(contract.Scalar(item["Stage"])) {
	case "qualification":
		return "qualification"
	case "needs analysis", "value proposition", "identify decision makers", "proposal/price quote":
		return "proposal"
	case "negotiation/review":
		return "negotiation"
	case "closed won":
		return "won"
	case "closed lost", "closed-lost to competition", "closed lost to competition":
		return "lost"
	}
	return ""
}

// newestModified is the change mark: the latest Modified_Time in a list
// as RFC 3339 in UTC, which If-Modified-Since reads back, or the previous
// mark when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["Modified_Time"])); err == nil && parsed.After(newest) {
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
