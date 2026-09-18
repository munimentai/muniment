// Package dynamics reads one Dynamics 365 Sales environment behind the
// reader contract: four objects, accounts, contacts, leads and
// opportunities, each flattened to text fields a mapping can point at. It
// uses the Dataverse Web API over net/http alone, signs in once per process
// through an Azure AD app registration's client credentials, and writes
// nothing back. Backfill walks an OData query through its next link, and
// Delta is one query for the newest change after the cursor's mark.
package dynamics

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultLoginURL is the Azure AD host every tenant signs in through. A
// test points the reader elsewhere, and the environment then follows the
// same host.
const DefaultLoginURL = "https://login.microsoftonline.com"

// apiPath is where the Web API sits under the environment's URL.
const apiPath = "/api/data/v9.2"

// PageLimit is the most records one page returns, the largest page size
// Dataverse honors.
const PageLimit = 5000

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// formatted is the annotation that carries an option set's label or a
// lookup's display name beside the raw value.
const formatted = "@OData.Community.Display.V1.FormattedValue"

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

// Source is one Dataverse environment reached through one app registration.
type Source struct {
	loginURL     string
	orgURL       string
	scope        string
	tenantID     string
	clientID     string
	clientSecret string
	client       *http.Client
	token        string
}

// New opens a source on the credentials the panel packs: the directory's
// tenant id, the app registration's client id and client secret, and the
// environment URL such as https://acme.crm.dynamics.com. An empty base URL
// is the live login host and that environment. A base URL points both at
// one fake, and the scope still names the environment.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	orgURL := strings.TrimRight(credentials["org_url"], "/")
	loginURL := DefaultLoginURL
	if baseURL != "" {
		loginURL = strings.TrimRight(baseURL, "/")
		orgURL = loginURL
	}
	scope := ""
	if org := strings.TrimRight(credentials["org_url"], "/"); org != "" {
		scope = org + "/.default"
	}
	return &Source{
		loginURL:     loginURL,
		orgURL:       orgURL,
		scope:        scope,
		tenantID:     credentials["tenant_id"],
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		client:       &http.Client{Timeout: 60 * time.Second},
	}
}

type record = map[string]any

// object is one table: its entity set, its primary key, the attributes the
// query selects and how each column reads.
type object struct {
	name   string
	label  string
	set    string
	key    string
	fields []field
}

type field struct {
	name  string
	guess string
	// attribute names the column the query selects, empty for a derived
	// column.
	attribute string
	read      func(record) string
}

var objects = []object{
	{
		name: "accounts", label: "Accounts", set: "accounts", key: "accountid",
		fields: []field{
			{"id", "id", "accountid", text("accountid")},
			{"name", "string", "name", text("name")},
			{"website", "string", "websiteurl", text("websiteurl")},
			{"domain", "domain", "", websiteDomain},
			{"email", "email", "emailaddress1", text("emailaddress1")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "telephone1", text("telephone1")},
			{"industry", "string", "industrycode", label("industrycode")},
			{"type", "string", "customertypecode", label("customertypecode")},
			{"status", "string", "statecode", lowered(label("statecode"))},
			{"employees", "number", "numberofemployees", text("numberofemployees")},
			{"annual_revenue", "number", "revenue", money("revenue")},
			{"currency_id", "id", "_transactioncurrencyid_value", text("_transactioncurrencyid_value")},
			{"currency", "string", "", label("_transactioncurrencyid_value")},
			{"city", "string", "address1_city", text("address1_city")},
			{"state", "string", "address1_stateorprovince", text("address1_stateorprovince")},
			{"postal_code", "string", "address1_postalcode", text("address1_postalcode")},
			{"country", "string", "address1_country", text("address1_country")},
			{"parent_id", "id", "_parentaccountid_value", text("_parentaccountid_value")},
			{"primary_contact_id", "id", "_primarycontactid_value", text("_primarycontactid_value")},
			{"owner_id", "id", "_ownerid_value", text("_ownerid_value")},
			{"description", "string", "description", text("description")},
			{"created", "date-time", "createdon", when("createdon")},
			{"modified", "date-time", "modifiedon", when("modifiedon")},
		},
	},
	{
		name: "contacts", label: "Contacts", set: "contacts", key: "contactid",
		fields: []field{
			{"id", "id", "contactid", text("contactid")},
			{"first_name", "string", "firstname", text("firstname")},
			{"last_name", "string", "lastname", text("lastname")},
			{"name", "string", "fullname", text("fullname")},
			{"email", "email", "emailaddress1", text("emailaddress1")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "telephone1", text("telephone1")},
			{"mobile", "phone", "mobilephone", text("mobilephone")},
			{"job_title", "string", "jobtitle", text("jobtitle")},
			{"account_id", "id", "_parentcustomerid_value", text("_parentcustomerid_value")},
			{"account_name", "string", "", label("_parentcustomerid_value")},
			{"status", "string", "statecode", lowered(label("statecode"))},
			{"city", "string", "address1_city", text("address1_city")},
			{"state", "string", "address1_stateorprovince", text("address1_stateorprovince")},
			{"country", "string", "address1_country", text("address1_country")},
			{"owner_id", "id", "_ownerid_value", text("_ownerid_value")},
			{"created", "date-time", "createdon", when("createdon")},
			{"modified", "date-time", "modifiedon", when("modifiedon")},
		},
	},
	{
		name: "leads", label: "Leads", set: "leads", key: "leadid",
		fields: []field{
			{"id", "id", "leadid", text("leadid")},
			{"name", "string", "fullname", text("fullname")},
			{"first_name", "string", "firstname", text("firstname")},
			{"last_name", "string", "lastname", text("lastname")},
			{"company", "string", "companyname", text("companyname")},
			{"email", "email", "emailaddress1", text("emailaddress1")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "telephone1", text("telephone1")},
			{"job_title", "string", "jobtitle", text("jobtitle")},
			{"subject", "string", "subject", text("subject")},
			{"status", "string", "statecode", lowered(label("statecode"))},
			{"status_reason", "string", "statuscode", label("statuscode")},
			{"rating", "string", "leadqualitycode", lowered(label("leadqualitycode"))},
			{"lead_source", "string", "leadsourcecode", label("leadsourcecode")},
			{"account_id", "id", "_parentaccountid_value", text("_parentaccountid_value")},
			{"contact_id", "id", "_parentcontactid_value", text("_parentcontactid_value")},
			{"owner_id", "id", "_ownerid_value", text("_ownerid_value")},
			{"created", "date-time", "createdon", when("createdon")},
			{"modified", "date-time", "modifiedon", when("modifiedon")},
		},
	},
	{
		name: "opportunities", label: "Opportunities", set: "opportunities", key: "opportunityid",
		fields: []field{
			{"id", "id", "opportunityid", text("opportunityid")},
			{"name", "string", "name", text("name")},
			{"amount", "number", "estimatedvalue", money("estimatedvalue")},
			{"actual_amount", "number", "actualvalue", money("actualvalue")},
			{"currency_id", "id", "_transactioncurrencyid_value", text("_transactioncurrencyid_value")},
			{"currency", "string", "", label("_transactioncurrencyid_value")},
			{"stage_name", "string", "salesstage", label("salesstage")},
			{"stage", "string", "statecode", opportunityStage},
			{"status_reason", "string", "statuscode", label("statuscode")},
			{"probability", "number", "closeprobability", text("closeprobability")},
			{"close_date", "date", "estimatedclosedate", text("estimatedclosedate")},
			{"actual_close_date", "date", "actualclosedate", text("actualclosedate")},
			{"account_id", "id", "_parentaccountid_value", text("_parentaccountid_value")},
			{"contact_id", "id", "_parentcontactid_value", text("_parentcontactid_value")},
			{"customer_id", "id", "_customerid_value", text("_customerid_value")},
			{"owner_id", "id", "_ownerid_value", text("_ownerid_value")},
			{"description", "string", "description", text("description")},
			{"created", "date-time", "createdon", when("createdon")},
			{"modified", "date-time", "modifiedon", when("modifiedon")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Dynamics 365 has no %s to read. The objects are accounts, contacts, leads and opportunities.", name)}
}

// Objects signs in, proves the environment with one small query, and lists
// the four.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if err := s.signIn(); err != nil {
		return nil, err
	}
	if _, _, _, err := s.query(objects[0].odata("", "", 1), 1); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// The query asks for its count, so Counted is true when Dataverse names
// one.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, total, _, err := s.query(object.odata("", "", 0), sampleRows)
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
	counted := total >= 0
	if !counted {
		total = len(rows)
	}
	return &Description{Source: "dynamics", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: total, Hash: newestModified(items, ""), Counted: counted}, nil
}

// Page reads one page: the first through the query, the rest through the
// next link the previous answer named. Total is the query's own count when
// Dataverse names one, else the rows seen so far.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	offset, hash, link := 0, "", ""
	if cursor != nil {
		offset, hash, link = cursor.Offset, cursor.Hash, cursor.Token
	}
	if link == "" {
		link = object.odata("", "", 0)
	}
	items, total, next, err := s.query(link, limit)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestModified(items, hash)
	counted := total >= 0
	if !counted {
		total = offset + len(rows)
	}
	page := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: counted}
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
	filter := ""
	if mark != "" {
		filter = "modifiedon gt " + mark
	}
	items, _, _, err := s.query(object.odata(filter, "modifiedon desc", 1), 1)
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

// odata builds the object's query path: the attributes it selects, the
// filter and order asked for, a stable order by key otherwise so the next
// link walks every record once, and the count when the query walks. A top
// above zero reads that many records and no more.
func (o *object) odata(filter, order string, top int) string {
	names := []string{}
	seen := map[string]bool{}
	for _, f := range o.fields {
		if f.attribute != "" && !seen[f.attribute] {
			seen[f.attribute] = true
			names = append(names, f.attribute)
		}
	}
	query := url.Values{"$select": {strings.Join(names, ",")}}
	if filter != "" {
		query.Set("$filter", filter)
	}
	if order == "" {
		order = o.key + " asc"
	}
	query.Set("$orderby", order)
	if top > 0 {
		query.Set("$top", strconv.Itoa(top))
	} else {
		query.Set("$count", "true")
	}
	return fmt.Sprintf("%s/%s?%s", apiPath, o.set, query.Encode())
}

// query runs one OData read, a path under the environment or the absolute
// next link, with the page size in the Prefer header. The count is the
// query's own when the answer names one, else minus one.
func (s *Source) query(link string, limit int) ([]record, int, string, error) {
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	target := link
	if !strings.HasPrefix(target, "http") {
		target = s.orgURL + target
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, "", &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("OData-MaxVersion", "4.0")
	request.Header.Set("OData-Version", "4.0")
	request.Header.Set("Prefer", fmt.Sprintf("odata.maxpagesize=%d,odata.include-annotations=\"OData.Community.Display.V1.FormattedValue\"", limit))
	body, status, err := s.do(request)
	if err != nil {
		return nil, 0, "", err
	}
	if err := refusal(status, body); err != nil {
		return nil, 0, "", err
	}
	var answer struct {
		Count    *int     `json:"@odata.count"`
		NextLink string   `json:"@odata.nextLink"`
		Value    []record `json:"value"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, 0, "", &Failure{Code: "source", Message: fmt.Sprintf("Dynamics 365's answer does not parse: %s.", err)}
	}
	total := -1
	if answer.Count != nil {
		total = *answer.Count
	}
	return answer.Value, total, answer.NextLink, nil
}

// signIn exchanges the app registration's credentials for an access token
// once, scoped to the environment.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.tenantID == "" || s.clientID == "" || s.clientSecret == "" || s.scope == "" {
		return &Failure{Code: "not_connected", Message: "Connect Dynamics 365 with its tenant id, client id, client secret and environment URL first."}
	}
	form := url.Values{"grant_type": {"client_credentials"}, "client_id": {s.clientID}, "client_secret": {s.clientSecret}, "scope": {s.scope}}
	request, err := http.NewRequest(http.MethodPost, fmt.Sprintf("%s/%s/oauth2/v2.0/token", s.loginURL, url.PathEscape(s.tenantID)), strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Microsoft refused the app registration: %s. Connect Dynamics 365 again with the tenant id, client id and client secret of an app registration that has an application user in the environment.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Microsoft answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Microsoft's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Dynamics 365 did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Dynamics 365's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Dataverse answers into the one failure the
// runtime reads. A 403 is an application user without read privileges on
// the table.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Dynamics 365 refused the access token. Connect Dynamics 365 again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Dynamics 365 refused the request: %s. The app's application user needs a security role with read privileges on the table.", apiMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Dynamics 365 asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Dynamics 365 answered %d: %s", status, apiMessage(body))}
	}
	return nil
}

func apiMessage(body []byte) string {
	var failure struct {
		Error struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error.Message != "" {
		return failure.Error.Message
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

func oauthMessage(body []byte) string {
	var failure struct {
		Error       string `json:"error"`
		Description string `json:"error_description"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error != "" {
		if failure.Description != "" {
			return failure.Error + ", " + contract.Clip(failure.Description, 200)
		}
		return failure.Error
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

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

// label reads an option set's label or a lookup's display name from the
// formatted value annotation, else the raw value.
func label(key string) func(record) string {
	return func(item record) string {
		if value := contract.Scalar(item[key+formatted]); value != "" {
			return value
		}
		if strings.HasPrefix(key, "_") {
			return ""
		}
		return contract.Scalar(item[key])
	}
}

func lowered(read func(record) string) func(record) string {
	return func(item record) string { return strings.ToLower(read(item)) }
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Dataverse's `2023-11-15T11:00:00Z` as RFC 3339 in UTC.
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

// money reads a number Dataverse writes in the major unit as a decimal
// with two places.
func money(key string) func(record) string {
	return func(item record) string {
		number, ok := item[key].(float64)
		if !ok {
			return ""
		}
		return strconv.FormatFloat(number, 'f', 2, 64)
	}
}

func emailDomain(item record) string {
	email := contract.Scalar(item["emailaddress1"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// websiteDomain reads a website as its registrable host, without scheme,
// `www.` or path.
func websiteDomain(item record) string {
	site := strings.TrimSpace(contract.Scalar(item["websiteurl"]))
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

// opportunityStage folds the state and the sales stage onto the deal
// kind's stages: a won or lost state first, then the pipeline's four
// stages by label. A stage the reader cannot place reads as empty, and the
// mapping keeps the stage name beside it.
func opportunityStage(item record) string {
	switch strings.ToLower(label("statecode")(item)) {
	case "won":
		return "won"
	case "lost":
		return "lost"
	}
	switch strings.ToLower(label("salesstage")(item)) {
	case "qualify":
		return "qualification"
	case "develop":
		return "discovery"
	case "propose":
		return "proposal"
	case "close":
		return "negotiation"
	}
	return ""
}

// newestModified is the change mark: the latest modifiedon in a list as
// RFC 3339 in UTC, the shape the filter reads back, or the previous mark
// when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["modifiedon"])); err == nil && parsed.After(newest) {
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
