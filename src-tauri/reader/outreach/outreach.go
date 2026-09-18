// Package outreach reads an Outreach org behind the reader contract: four
// objects, prospects, accounts, sequences and mailboxes, each flattened to
// text fields a mapping can point at. It uses the v2 JSON:API over
// net/http alone, trades the refresh token for an access token once per
// process, and writes nothing back. Backfill walks cursor pagination, and
// Delta is one filtered list for the newest change after the cursor's
// mark.
package outreach

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

// DefaultBaseURL is Outreach's API host. A test points the reader
// elsewhere.
const DefaultBaseURL = "https://api.outreach.io"

// PageLimit is the most records one Outreach list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// customFieldCount is how many custom attributes Outreach gives a prospect
// or an account, named custom1 through custom150.
const customFieldCount = 150

// markLayout is how the change mark is written, with the milliseconds
// Outreach keeps, so a range filter reads it back.
const markLayout = "2006-01-02T15:04:05.000Z"

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

// Source is one Outreach org reached through one OAuth app.
type Source struct {
	baseURL      string
	clientID     string
	clientSecret string
	refreshToken string
	client       *http.Client
	token        string
}

// New opens a source on the credentials the panel packs: the client id,
// the client secret and the refresh token. An empty base URL is the live
// API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		baseURL:      strings.TrimRight(baseURL, "/"),
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		refreshToken: credentials["refresh_token"],
		client:       &http.Client{Timeout: 30 * time.Second},
	}
}

// record is one JSON:API resource: its id, its attributes and the
// relationships that name other records.
type record struct {
	ID            any            `json:"id"`
	Attributes    map[string]any `json:"attributes"`
	Relationships map[string]struct {
		Data any `json:"data"`
	} `json:"relationships"`
}

// object is one Outreach resource: its list path, whether the org's
// custom attributes ride on it, and the core fields in the order Describe
// lists them.
type object struct {
	name   string
	label  string
	path   string
	custom bool
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
		name:   "prospects",
		label:  "Prospects",
		path:   "/api/v2/prospects",
		custom: true,
		fields: []field{
			{"id", "id", id},
			{"first_name", "string", attr("firstName")},
			{"last_name", "string", attr("lastName")},
			{"name", "string", prospectName},
			{"email", "email", first("emails")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", firstPhone},
			{"job_title", "string", attr("title")},
			{"company", "string", attr("company")},
			{"account_id", "id", related("account")},
			{"owner_id", "id", related("owner")},
			{"stage_id", "id", related("stage")},
			{"tags", "string", tags("tags")},
			{"opted_out", "boolean", boolean("optedOut")},
			{"engaged_at", "date-time", when("engagedAt")},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("updatedAt")},
		},
	},
	{
		name:   "accounts",
		label:  "Accounts",
		path:   "/api/v2/accounts",
		custom: true,
		fields: []field{
			{"id", "id", id},
			{"name", "string", attr("name")},
			{"domain", "domain", attr("domain")},
			{"website", "string", attr("websiteUrl")},
			{"industry", "string", attr("industry")},
			{"type", "string", attr("companyType")},
			{"employees", "number", attr("numberOfEmployees")},
			{"city", "string", attr("locality")},
			{"description", "string", attr("description")},
			{"owner_id", "id", related("owner")},
			{"tags", "string", tags("tags")},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("updatedAt")},
		},
	},
	{
		name:  "sequences",
		label: "Sequences",
		path:  "/api/v2/sequences",
		fields: []field{
			{"id", "id", id},
			{"name", "string", attr("name")},
			{"description", "string", attr("description")},
			{"enabled", "boolean", boolean("enabled")},
			{"type", "string", attr("sequenceType")},
			{"share_type", "string", attr("shareType")},
			{"steps", "number", attr("sequenceStepCount")},
			{"deliver_count", "number", attr("deliverCount")},
			{"open_count", "number", attr("openCount")},
			{"reply_count", "number", attr("replyCount")},
			{"bounce_count", "number", attr("bounceCount")},
			{"owner_id", "id", related("owner")},
			{"tags", "string", tags("tags")},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("updatedAt")},
		},
	},
	{
		name:  "mailboxes",
		label: "Mailboxes",
		path:  "/api/v2/mailboxes",
		fields: []field{
			{"id", "id", id},
			{"email", "email", attr("email")},
			{"email_domain", "domain", emailDomain},
			{"user_name", "string", attr("userName")},
			{"provider", "string", attr("provider")},
			{"send_disabled", "boolean", boolean("sendDisabled")},
			{"sync_disabled", "boolean", boolean("syncDisabled")},
			{"user_id", "id", related("user")},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("updatedAt")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Outreach has no %s to read. The objects are prospects, accounts, sequences and mailboxes.", name)}
}

// Objects trades the refresh token, which proves the app, and lists the
// four objects.
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
// samples. The custom attributes filled on that page are columns too.
// The list names no count under cursor pagination, so Rows is the rows
// read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, _, err := s.list(object.path, pageQuery(sampleRows, ""))
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
	if object.custom {
		columns = append(columns, customColumns(rows)...)
	}
	return &Description{
		Source:  "outreach",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestUpdated(items, ""),
		Counted: false,
	}, nil
}

// Page reads one list page after the cursor's token. Total is the rows
// seen so far, because the list names no count.
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
	offset, hash, after := 0, "", ""
	if cursor != nil {
		offset, hash, after = cursor.Offset, cursor.Hash, cursor.Token
	}
	items, next, err := s.list(object.path, pageQuery(limit, after))
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

// Delta asks for the one record updated at or after the mark, newest
// first. A record at the mark itself answers unchanged.
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
	query := url.Values{"page[size]": {"1"}, "sort": {"-updatedAt"}, "count": {"false"}}
	if mark != "" {
		query.Set("filter[updatedAt]", mark+"..inf")
	}
	items, _, err := s.list(object.path, query)
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

// pageQuery builds one cursor page: a stable sort, no count, and the
// token the previous answer named.
func pageQuery(limit int, after string) url.Values {
	query := url.Values{"page[size]": {strconv.Itoa(limit)}, "sort": {"id"}, "count": {"false"}}
	if after != "" {
		query.Set("page[after]", after)
	}
	return query
}

// signIn trades the refresh token for an access token once per process.
// Outreach answers a fresh refresh token beside it, and the runtime holds
// the secret, so the reader keeps using the one it was given.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" || s.refreshToken == "" {
		return &Failure{Code: "not_connected", Message: "Connect Outreach with its client id, client secret and refresh token first."}
	}
	form := url.Values{
		"grant_type":    {"refresh_token"},
		"client_id":     {s.clientID},
		"client_secret": {s.clientSecret},
		"refresh_token": {s.refreshToken},
	}
	request, err := http.NewRequest(http.MethodPost, s.baseURL+"/oauth/token", strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Outreach refused the app: %s. Connect Outreach again with the client id, client secret and a refresh token the app issued.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Outreach answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Outreach's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

// list fetches one page of a resource and answers its records and the
// cursor of the next page, empty when the collection ends.
func (s *Source) list(path string, query url.Values) ([]record, string, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, "", &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	body, status, err := s.do(request)
	if err != nil {
		return nil, "", err
	}
	if err := refusal(status, body); err != nil {
		return nil, "", err
	}
	var listed struct {
		Data  []record `json:"data"`
		Links struct {
			Next string `json:"next"`
		} `json:"links"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Outreach's answer does not parse: %s.", err)}
	}
	return listed.Data, nextCursor(listed.Links.Next), nil
}

// nextCursor reads the page[after] token out of the next link, so the
// cursor holds the token and never the host.
func nextCursor(link string) string {
	if link == "" {
		return ""
	}
	parsed, err := url.Parse(link)
	if err != nil {
		return ""
	}
	return parsed.Query().Get("page[after]")
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/vnd.api+json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Outreach did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Outreach's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Outreach answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Outreach refused the access token. Connect Outreach again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Outreach refused the request: %s. The app needs the read scope of the object.", outreachMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Outreach asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Outreach answered %d: %s", status, outreachMessage(body))}
	}
	return nil
}

func outreachMessage(body []byte) string {
	var failure struct {
		Errors []struct {
			Title  string `json:"title"`
			Detail string `json:"detail"`
		} `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Errors) > 0 {
		if failure.Errors[0].Detail != "" {
			return failure.Errors[0].Detail
		}
		return failure.Errors[0].Title
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

// row flattens one record: the core fields, then every filled custom
// attribute as its own column under Outreach's own name.
func (o *object) row(item record) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	if o.custom {
		for index := 1; index <= customFieldCount; index++ {
			key := "custom" + strconv.Itoa(index)
			if value := contract.Scalar(item.Attributes[key]); value != "" {
				row[key] = value
			}
		}
	}
	return row
}

// customColumns names the custom attributes the sampled rows fill, in
// their numbered order.
func customColumns(rows []Row) []contract.Column {
	numbers := []int{}
	seen := map[int]bool{}
	for _, row := range rows {
		for key := range row {
			if !strings.HasPrefix(key, "custom") {
				continue
			}
			number, err := strconv.Atoi(strings.TrimPrefix(key, "custom"))
			if err != nil || seen[number] {
				continue
			}
			seen[number] = true
			numbers = append(numbers, number)
		}
	}
	sort.Ints(numbers)
	columns := make([]contract.Column, 0, len(numbers))
	for _, number := range numbers {
		columns = append(columns, contract.Column{Name: "custom" + strconv.Itoa(number), Guess: "string"})
	}
	return columns
}

func id(item record) string { return contract.Scalar(item.ID) }

func attr(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item.Attributes[key]) }
}

// first reads the first entry of a list attribute.
func first(key string) func(record) string {
	return func(item record) string {
		list, ok := item.Attributes[key].([]any)
		if !ok {
			return contract.Scalar(item.Attributes[key])
		}
		if len(list) == 0 {
			return ""
		}
		return contract.Scalar(list[0])
	}
}

// firstPhone reads a prospect's first work phone, else the first mobile,
// else the first home phone.
func firstPhone(item record) string {
	for _, key := range []string{"workPhones", "mobilePhones", "homePhones"} {
		if phone := first(key)(item); phone != "" {
			return phone
		}
	}
	return ""
}

func prospectName(item record) string {
	if name := strings.TrimSpace(contract.Scalar(item.Attributes["name"])); name != "" {
		return name
	}
	name := strings.TrimSpace(strings.TrimSpace(contract.Scalar(item.Attributes["firstName"])) + " " + strings.TrimSpace(contract.Scalar(item.Attributes["lastName"])))
	if name != "" {
		return name
	}
	return first("emails")(item)
}

func emailDomain(item record) string {
	email := first("emails")(item)
	if email == "" {
		email = contract.Scalar(item.Attributes["email"])
	}
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// related reads the id of a to-one relationship.
func related(key string) func(record) string {
	return func(item record) string {
		relationship, ok := item.Relationships[key]
		if !ok {
			return ""
		}
		if data, ok := relationship.Data.(map[string]any); ok {
			return contract.Scalar(data["id"])
		}
		return ""
	}
}

func tags(key string) func(record) string {
	return func(item record) string { return contract.Joined(item.Attributes[key], true) }
}

func boolean(key string) func(record) string {
	return func(item record) string {
		switch value := item.Attributes[key].(type) {
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
	return func(item record) string { return formatTime(contract.Scalar(item.Attributes[key])) }
}

// formatTime reads Outreach's ISO timestamp as RFC 3339 in UTC.
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

// newestUpdated is the change mark: the latest updatedAt in a list with
// its milliseconds, so a range filter reads it back, or the previous mark
// when nothing newer appears.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item.Attributes["updatedAt"])); err == nil && parsed.After(newest) {
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
