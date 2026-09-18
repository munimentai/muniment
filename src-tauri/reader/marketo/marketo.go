// Package marketo reads one Marketo instance behind the reader contract:
// three objects, leads, companies and programs, each flattened to text
// fields a mapping can point at. It uses the REST API over net/http alone,
// signs in once per process through the identity endpoint's client
// credentials, and writes nothing back. Backfill walks leads and companies
// through the paging token the activities endpoint issues, and programs by
// offset. Delta is one query for records changed after the cursor's mark:
// a paging token since the mark for leads and companies, and the updated
// range for programs.
package marketo

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

// hostSuffix follows the Munchkin id in the instance's REST host.
const hostSuffix = ".mktorest.com"

// PageLimit is the most leads or companies one batch returns.
const PageLimit = 300

// ProgramPageLimit is the most programs one page returns.
const ProgramPageLimit = 200

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// floor is the time the first paging token starts from, before any Marketo
// record.
const floor = "2000-01-01T00:00:00Z"

// stamp is the time form the paging token and program filters read.
const stamp = "2006-01-02T15:04:05Z07:00"

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

// Source is one Marketo instance reached through one API user's custom
// service.
type Source struct {
	baseURL      string
	clientID     string
	clientSecret string
	client       *http.Client
	token        string
	now          func() time.Time
}

// New opens a source on the credentials the panel packs: the custom
// service's client id and client secret, and the Munchkin id that forms
// the instance's host. An empty base URL is that host.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		if munchkin := strings.ToLower(credentials["munchkin_id"]); munchkin != "" {
			baseURL = "https://" + munchkin + hostSuffix
		}
	}
	return &Source{
		baseURL:      strings.TrimRight(baseURL, "/"),
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		client:       &http.Client{Timeout: 60 * time.Second},
		now:          time.Now,
	}
}

type record = map[string]any

// object is one endpoint: its path, the lead database fields the query
// asks for, whether the paging token walks it, and how each column reads.
type object struct {
	name   string
	label  string
	path   string
	paged  bool
	fields []field
}

type field struct {
	name  string
	guess string
	// attribute names the lead database field the query asks for, empty for
	// a derived column or an asset field.
	attribute string
	read      func(record) string
}

var objects = []object{
	{
		name: "leads", label: "Leads", path: "/rest/v1/leads.json", paged: true,
		fields: []field{
			{"id", "id", "id", text("id")},
			{"first_name", "string", "firstName", text("firstName")},
			{"last_name", "string", "lastName", text("lastName")},
			{"email", "email", "email", text("email")},
			{"email_domain", "domain", "", emailDomain},
			{"phone", "phone", "phone", text("phone")},
			{"mobile", "phone", "mobilePhone", text("mobilePhone")},
			{"company", "string", "company", text("company")},
			{"job_title", "string", "title", text("title")},
			{"lead_status", "string", "leadStatus", text("leadStatus")},
			{"lead_source", "string", "leadSource", text("leadSource")},
			{"industry", "string", "industry", text("industry")},
			{"score", "number", "leadScore", text("leadScore")},
			{"website", "string", "website", text("website")},
			{"domain", "domain", "", websiteDomain},
			{"city", "string", "city", text("city")},
			{"state", "string", "state", text("state")},
			{"postal_code", "string", "postalCode", text("postalCode")},
			{"country", "string", "country", text("country")},
			{"unsubscribed", "boolean", "unsubscribed", boolean("unsubscribed")},
			{"created", "date-time", "createdAt", when("createdAt")},
			{"modified", "date-time", "updatedAt", when("updatedAt")},
		},
	},
	{
		name: "companies", label: "Companies", path: "/rest/v1/companies.json", paged: true,
		fields: []field{
			{"id", "id", "id", text("id")},
			{"name", "string", "company", text("company")},
			{"external_id", "id", "externalCompanyId", text("externalCompanyId")},
			{"website", "string", "website", text("website")},
			{"domain", "domain", "", websiteDomain},
			{"industry", "string", "industry", text("industry")},
			{"annual_revenue", "number", "annualRevenue", number("annualRevenue")},
			{"employees", "number", "numberOfEmployees", text("numberOfEmployees")},
			{"phone", "phone", "mainPhone", text("mainPhone")},
			{"sic_code", "string", "sicCode", text("sicCode")},
			{"street", "string", "billingStreet", text("billingStreet")},
			{"city", "string", "billingCity", text("billingCity")},
			{"state", "string", "billingState", text("billingState")},
			{"postal_code", "string", "billingPostalCode", text("billingPostalCode")},
			{"country", "string", "billingCountry", text("billingCountry")},
			{"created", "date-time", "createdAt", when("createdAt")},
			{"modified", "date-time", "updatedAt", when("updatedAt")},
		},
	},
	{
		name: "programs", label: "Programs", path: "/rest/asset/v1/programs.json",
		fields: []field{
			{"id", "id", "", text("id")},
			{"name", "string", "", text("name")},
			{"description", "string", "", text("description")},
			{"type", "string", "", lowered("type")},
			{"channel", "string", "", text("channel")},
			{"status", "string", "", lowered("status")},
			{"workspace", "string", "", text("workspace")},
			{"folder_id", "id", "", nested("folder", "value")},
			{"folder_name", "string", "", nested("folder", "folderName")},
			{"url", "string", "", text("url")},
			{"tags", "string", "", tags},
			{"cost", "number", "", cost},
			{"start_date", "date", "", day("startDate")},
			{"end_date", "date", "", day("endDate")},
			{"created", "date-time", "", when("createdAt")},
			{"modified", "date-time", "", when("updatedAt")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Marketo has no %s to read. The objects are leads, companies and programs.", name)}
}

// Objects signs in, which proves the client id and secret, and lists the
// three.
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

// Describe reads the first page and answers the fields with their samples.
// Marketo counts nothing, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	page, err := s.Page(name, nil, sampleRows)
	if err != nil {
		return nil, err
	}
	columns := make([]contract.Column, 0, len(object.fields))
	for _, f := range object.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	return &Description{Source: "marketo", Object: name, Label: object.label, Fields: contract.Sample(columns, page.Rows), Rows: len(page.Rows), Hash: page.Hash, Counted: false}, nil
}

// Page reads one batch after the cursor's token: the next page token for
// leads and companies, a paging token since the floor when the cursor
// names none, and the offset for programs. Total is the rows seen so far.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	offset, hash, token := 0, "", ""
	if cursor != nil {
		offset, hash, token = cursor.Offset, cursor.Hash, cursor.Token
	}
	var items []record
	var next string
	if object.paged {
		if token == "" {
			token, err = s.pagingToken(floor)
			if err != nil {
				return nil, err
			}
		}
		items, next, err = s.batch(object, token, limit)
	} else {
		items, next, err = s.programs(token, limit, "")
	}
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta asks for the one record changed after the mark: leads and
// companies through a paging token since the mark, programs through the
// updated range from the mark to now.
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
	since := mark
	if since == "" {
		since = floor
	}
	var items []record
	if object.paged {
		token, err := s.pagingToken(since)
		if err != nil {
			return nil, err
		}
		items, _, err = s.batch(object, token, 1)
		if err != nil {
			return nil, err
		}
	} else {
		items, _, err = s.programs("", 1, since)
		if err != nil {
			return nil, err
		}
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

// pagingToken asks the activities endpoint for the token that starts at a
// time.
func (s *Source) pagingToken(since string) (string, error) {
	body, err := s.get("/rest/v1/activities/pagingtoken.json", url.Values{"sinceDatetime": {since}})
	if err != nil {
		return "", err
	}
	var answer struct {
		NextPageToken string `json:"nextPageToken"`
	}
	if json.Unmarshal(body, &answer) != nil || answer.NextPageToken == "" {
		return "", &Failure{Code: "source", Message: "Marketo's paging token answer carried no token."}
	}
	return answer.NextPageToken, nil
}

// batch reads one batch of leads or companies after a paging token, with
// the fields the object asks for, and answers the token for the next batch
// when more follow.
func (s *Source) batch(object *object, token string, limit int) ([]record, string, error) {
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	names := []string{}
	for _, f := range object.fields {
		if f.attribute != "" {
			names = append(names, f.attribute)
		}
	}
	query := url.Values{"nextPageToken": {token}, "batchSize": {strconv.Itoa(limit)}, "fields": {strings.Join(names, ",")}}
	body, err := s.get(object.path, query)
	if err != nil {
		return nil, "", err
	}
	var answer struct {
		Result        []record `json:"result"`
		NextPageToken string   `json:"nextPageToken"`
		MoreResult    bool     `json:"moreResult"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Marketo's answer does not parse: %s.", err)}
	}
	next := ""
	if answer.MoreResult && answer.NextPageToken != "" {
		next = answer.NextPageToken
	}
	return answer.Result, next, nil
}

// programs reads one page of programs from an offset, only those updated
// after since when it is set, and answers the next offset when the page is
// full.
func (s *Source) programs(token string, limit int, since string) ([]record, string, error) {
	if limit <= 0 || limit > ProgramPageLimit {
		limit = ProgramPageLimit
	}
	offset := 0
	if token != "" {
		parsed, err := strconv.Atoi(token)
		if err != nil || parsed < 0 {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("The program cursor %q does not parse.", token)}
		}
		offset = parsed
	}
	query := url.Values{"offset": {strconv.Itoa(offset)}, "maxReturn": {strconv.Itoa(limit)}}
	if since != "" {
		query.Set("earliestUpdatedAt", since)
		query.Set("latestUpdatedAt", s.now().UTC().Format(stamp))
	}
	body, err := s.get(objects[2].path, query)
	if err != nil {
		return nil, "", err
	}
	var answer struct {
		Result []record `json:"result"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Marketo's answer does not parse: %s.", err)}
	}
	next := ""
	if len(answer.Result) == limit {
		next = strconv.Itoa(offset + limit)
	}
	return answer.Result, next, nil
}

// signIn exchanges the custom service's client id and secret for an access
// token once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.baseURL == "" || s.clientID == "" || s.clientSecret == "" {
		return &Failure{Code: "not_connected", Message: "Connect Marketo with its client id, client secret and Munchkin id first."}
	}
	query := url.Values{"grant_type": {"client_credentials"}, "client_id": {s.clientID}, "client_secret": {s.clientSecret}}
	request, err := http.NewRequest(http.MethodGet, s.baseURL+"/identity/oauth/token?"+query.Encode(), nil)
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest || status == http.StatusForbidden {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Marketo refused the client id and secret: %s. Connect Marketo again with the credentials of a custom service whose API user is still active.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Marketo answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Marketo's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

// get reads one path and answers its body once the status and the answer's
// own success flag pass.
func (s *Source) get(path string, query url.Values) ([]byte, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	body, status, err := s.do(request)
	if err != nil {
		return nil, err
	}
	if err := refusal(status, body); err != nil {
		return nil, err
	}
	return body, nil
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Marketo did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Marketo's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns what Marketo answers into the one failure the runtime
// reads. Marketo answers most refusals as a 200 whose body carries
// success false and an error code, so the code decides after the status:
// 601 and 602 are a bad or expired token, 603 is a permission the API
// user lacks, and 606, 607 and 615 are the rate, daily and concurrency
// limits.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized || status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: "Marketo refused the access token. Connect Marketo again."}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Marketo asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Marketo answered %d: %s", status, apiMessage(body))}
	}
	var answer struct {
		Success *bool `json:"success"`
		Errors  []struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"errors"`
	}
	if json.Unmarshal(body, &answer) != nil || answer.Success == nil || *answer.Success {
		return nil
	}
	code, message := "", "no message"
	if len(answer.Errors) > 0 {
		code, message = answer.Errors[0].Code, answer.Errors[0].Message
	}
	switch code {
	case "601", "602":
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Marketo refused the access token: %s. Connect Marketo again.", message)}
	case "603":
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Marketo refused the request: %s. The API user's role needs read access to the lead database and assets.", message)}
	case "606", "607", "615":
		return &Failure{Code: "rate_limited", Message: fmt.Sprintf("Marketo asked the reader to wait: %s. Run again in a minute.", message)}
	default:
		return &Failure{Code: "source", Message: fmt.Sprintf("Marketo answered error %s: %s", code, message)}
	}
}

func apiMessage(body []byte) string {
	var failure struct {
		Errors []struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Errors) > 0 {
		return failure.Errors[0].Code + ", " + failure.Errors[0].Message
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
			return failure.Error + ", " + failure.Description
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

func lowered(key string) func(record) string {
	return func(item record) string { return strings.ToLower(contract.Scalar(item[key])) }
}

func nested(parent, key string) func(record) string {
	return func(item record) string {
		if child, ok := item[parent].(map[string]any); ok {
			return contract.Scalar(child[key])
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

func when(key string) func(record) string {
	return func(item record) string {
		if parsed, ok := parseTime(contract.Scalar(item[key])); ok {
			return parsed.UTC().Format(time.RFC3339)
		}
		return ""
	}
}

// day reads a date, or the calendar day of a time.
func day(key string) func(record) string {
	return func(item record) string {
		value := strings.TrimSpace(contract.Scalar(item[key]))
		if len(value) >= 10 {
			return value[:10]
		}
		return value
	}
}

// number reads a number Marketo writes in the major unit as a decimal with
// two places.
func number(key string) func(record) string {
	return func(item record) string {
		switch value := item[key].(type) {
		case float64:
			return strconv.FormatFloat(value, 'f', 2, 64)
		case string:
			if parsed, err := strconv.ParseFloat(strings.TrimSpace(value), 64); err == nil {
				return strconv.FormatFloat(parsed, 'f', 2, 64)
			}
			return ""
		default:
			return ""
		}
	}
}

// tags reads a program's tags as `type:value` pairs joined by commas in a
// stable order.
func tags(item record) string {
	list, _ := item["tags"].([]any)
	pairs := make([]any, 0, len(list))
	for _, entry := range list {
		tag, _ := entry.(map[string]any)
		kind, value := contract.Scalar(tag["tagType"]), contract.Scalar(tag["tagValue"])
		if kind == "" || value == "" {
			continue
		}
		pairs = append(pairs, kind+":"+value)
	}
	return contract.Joined(pairs, true)
}

// cost sums a program's cost lines as a decimal with two places, empty
// when it has none.
func cost(item record) string {
	list, _ := item["costs"].([]any)
	total, found := 0.0, false
	for _, entry := range list {
		line, _ := entry.(map[string]any)
		if amount, ok := line["cost"].(float64); ok {
			total += amount
			found = true
		}
	}
	if !found {
		return ""
	}
	return strconv.FormatFloat(total, 'f', 2, 64)
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// websiteDomain reads a website as its registrable host, without scheme,
// `www.` or path.
func websiteDomain(item record) string {
	site := strings.TrimSpace(contract.Scalar(item["website"]))
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

// parseTime reads the two forms Marketo writes: `2023-11-15T11:00:00Z` on
// a lead, and `2023-11-15T11:00:00Z+0000` on an asset.
func parseTime(value string) (time.Time, bool) {
	value = strings.TrimSpace(value)
	if value == "" {
		return time.Time{}, false
	}
	for _, layout := range []string{time.RFC3339Nano, "2006-01-02T15:04:05Z-0700", "2006-01-02T15:04:05-0700"} {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed, true
		}
	}
	return time.Time{}, false
}

// newestUpdated is the change mark: the latest updatedAt in a list as RFC
// 3339 in UTC, the shape the paging token and the program filter read
// back, or the previous mark when nothing newer appears.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, ok := parseTime(contract.Scalar(item["updatedAt"])); ok && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, ok := parseTime(previous); ok && before.After(newest) {
		return previous
	}
	return newest.UTC().Format(time.RFC3339)
}
