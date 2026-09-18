// Package freshbooks reads a FreshBooks account behind the reader contract:
// three objects, clients, invoices and payments, each flattened to text
// fields a mapping can point at. It uses the accounting REST API over
// net/http alone, exchanges the app's refresh token for an access token once
// per process, and writes nothing back. Backfill walks numbered pages, and
// Delta is one query for the records updated after the cursor's mark.
package freshbooks

import (
	"bytes"
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

// DefaultBaseURL is FreshBooks' API host, which also serves the token
// endpoint. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.freshbooks.com"

// PageLimit is the most records one page returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// timeLayout is how FreshBooks writes a time, in the account's time zone.
const timeLayout = "2006-01-02 15:04:05"

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

// Source is one FreshBooks account reached through one app.
type Source struct {
	baseURL      string
	clientID     string
	clientSecret string
	refreshToken string
	accountID    string
	client       *http.Client
	token        string
}

// New opens a source on the credentials the panel packs: the app's client
// id, client secret and refresh token, and the account id every path names.
// An empty base URL is the live API.
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
		accountID:    credentials["account_id"],
		client:       &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one accounting endpoint: its path under the account, the key
// the result lists its records under, and how each field reads.
type object struct {
	name   string
	label  string
	path   string
	key    string
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "clients", label: "Clients", path: "/users/clients", key: "clients",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", clientName},
			{"organization", "string", text("organization")},
			{"first_name", "string", text("fname")},
			{"last_name", "string", text("lname")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", firstOf("bus_phone", "mob_phone", "home_phone")},
			{"street", "string", text("p_street")},
			{"city", "string", text("p_city")},
			{"state", "string", text("p_province")},
			{"postal_code", "string", text("p_code")},
			{"country", "string", text("p_country")},
			{"currency", "string", text("currency_code")},
			{"language", "string", text("language")},
			{"status", "string", visibility},
			{"created", "date-time", when("signup_date")},
			{"modified", "date-time", when("updated")},
		},
	},
	{
		name: "invoices", label: "Invoices", path: "/invoices/invoices", key: "invoices",
		fields: []field{
			{"id", "id", text("id")},
			{"number", "string", text("invoice_number")},
			{"client_id", "id", text("customerid")},
			{"client_name", "string", clientName},
			{"status", "string", text("v3_status")},
			{"amount", "number", money("amount")},
			{"amount_outstanding", "number", money("outstanding")},
			{"amount_paid", "number", money("paid")},
			{"currency", "string", currency("amount", "currency_code")},
			{"date", "date", text("create_date")},
			{"due_date", "date", text("due_date")},
			{"paid_at", "date", text("date_paid")},
			{"po_number", "string", text("po_number")},
			{"description", "string", text("description")},
			{"notes", "string", text("notes")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated")},
		},
	},
	{
		name: "payments", label: "Payments", path: "/payments/payments", key: "payments",
		fields: []field{
			{"id", "id", text("id")},
			{"invoice_id", "id", text("invoiceid")},
			{"client_id", "id", text("clientid")},
			{"amount", "number", money("amount")},
			{"currency", "string", currency("amount", "")},
			{"date", "date", text("date")},
			{"type", "string", text("type")},
			{"note", "string", text("note")},
			{"status", "string", visibility},
			{"modified", "date-time", when("updated")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("FreshBooks has no %s to read. The objects are clients, invoices and payments.", name)}
}

// Objects signs in, proves the account id with one small call, and lists
// the three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if err := s.signIn(); err != nil {
		return nil, err
	}
	if _, _, err := s.list(&objects[0], url.Values{"per_page": {"1"}, "page": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// The result names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, listed, err := s.list(object, url.Values{"per_page": {strconv.Itoa(sampleRows)}, "page": {"1"}})
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
	return &Description{Source: "freshbooks", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: listed.total, Hash: newestUpdated(items, ""), Counted: true}, nil
}

// Page reads one numbered page, the cursor's token or the first. Total is
// the result's own count.
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
	offset, hash, page := 0, "", "1"
	if cursor != nil {
		offset, hash = cursor.Offset, cursor.Hash
		if cursor.Token != "" {
			page = cursor.Token
		}
	}
	items, listed, err := s.list(object, url.Values{"per_page": {strconv.Itoa(limit)}, "page": {page}})
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(items, hash)
	answer := &Page{Rows: rows, Offset: offset, Total: listed.total, Hash: hash, Counted: true}
	if listed.page < listed.pages {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(listed.page + 1)}
	}
	return answer, nil
}

// Delta asks for the records updated after the mark and compares the
// newest with it. A first delta reads the first page.
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
	query := url.Values{"per_page": {strconv.Itoa(PageLimit)}, "page": {"1"}}
	if mark != "" {
		query.Set("search[updated_since]", mark)
	}
	items, _, err := s.list(object, query)
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

// signIn exchanges the app's refresh token for an access token once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" || s.refreshToken == "" || s.accountID == "" {
		return &Failure{Code: "not_connected", Message: "Connect FreshBooks with its client id, client secret, refresh token and account id first."}
	}
	grant, _ := json.Marshal(map[string]string{"grant_type": "refresh_token", "client_id": s.clientID, "client_secret": s.clientSecret, "refresh_token": s.refreshToken})
	request, err := http.NewRequest(http.MethodPost, s.baseURL+"/auth/oauth/token", bytes.NewReader(grant))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/json")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("FreshBooks refused the app: %s. Connect FreshBooks again with a refresh token that is still valid.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("FreshBooks answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "FreshBooks' sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

// listing is where one page stands in its result: the page, the page
// count and the record total.
type listing struct {
	page  int
	pages int
	total int
}

// list fetches one page of an accounting endpoint and answers its records
// and where the page stands.
func (s *Source) list(object *object, query url.Values) ([]record, listing, error) {
	target := fmt.Sprintf("%s/accounting/account/%s%s?%s", s.baseURL, url.PathEscape(s.accountID), object.path, query.Encode())
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, listing{}, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	body, status, err := s.do(request)
	if err != nil {
		return nil, listing{}, err
	}
	if err := refusal(object.name, status, body); err != nil {
		return nil, listing{}, err
	}
	var answer struct {
		Response struct {
			Result map[string]json.RawMessage `json:"result"`
		} `json:"response"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, listing{}, &Failure{Code: "source", Message: fmt.Sprintf("FreshBooks' answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := answer.Response.Result[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, listing{}, &Failure{Code: "source", Message: fmt.Sprintf("FreshBooks' %s do not parse: %s.", object.name, err)}
		}
	}
	listed := listing{page: number(answer.Response.Result["page"]), pages: number(answer.Response.Result["pages"]), total: number(answer.Response.Result["total"])}
	if listed.page == 0 {
		listed.page = 1
	}
	return items, listed, nil
}

func number(raw json.RawMessage) int {
	var value float64
	if json.Unmarshal(raw, &value) != nil {
		return 0
	}
	return int(value)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	request.Header.Set("Api-Version", "alpha")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("FreshBooks did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("FreshBooks' answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status FreshBooks answers into the one failure the
// runtime reads.
func refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "FreshBooks refused the access token. Connect FreshBooks again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("FreshBooks refused the request for %s: %s. The app needs read access to the account.", object, freshbooksMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "FreshBooks asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("FreshBooks answered %d: %s", status, freshbooksMessage(body))}
	}
	return nil
}

func freshbooksMessage(body []byte) string {
	var failure struct {
		Response struct {
			Errors []struct {
				Message string `json:"message"`
			} `json:"errors"`
		} `json:"response"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Response.Errors) > 0 && failure.Response.Errors[0].Message != "" {
		return failure.Response.Errors[0].Message
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

// firstOf reads the first of several keys that holds a value.
func firstOf(keys ...string) func(record) string {
	return func(item record) string {
		for _, key := range keys {
			if value := contract.Scalar(item[key]); value != "" {
				return value
			}
		}
		return ""
	}
}

// when reads a FreshBooks time as an ISO 8601 text without a zone, because
// FreshBooks writes it in the account's time zone. A date passes through.
func when(key string) func(record) string {
	return func(item record) string {
		value := strings.TrimSpace(contract.Scalar(item[key]))
		if parsed, err := time.Parse(timeLayout, value); err == nil {
			return parsed.Format("2006-01-02T15:04:05")
		}
		return value
	}
}

// money reads a FreshBooks amount, an object of a decimal text and a
// currency code, as its decimal in the major unit.
func money(key string) func(record) string {
	return func(item record) string {
		if amount, ok := item[key].(map[string]any); ok {
			return contract.Scalar(amount["amount"])
		}
		return ""
	}
}

// currency reads the code beside an amount, or the record's own currency
// when the amount carries none.
func currency(amountKey, fallback string) func(record) string {
	return func(item record) string {
		if amount, ok := item[amountKey].(map[string]any); ok {
			if code := contract.Scalar(amount["code"]); code != "" {
				return code
			}
		}
		if fallback != "" {
			return contract.Scalar(item[fallback])
		}
		return ""
	}
}

// clientName reads the organization, or the person's name when the client
// is a person.
func clientName(item record) string {
	if organization := strings.TrimSpace(contract.Scalar(item["organization"])); organization != "" {
		return organization
	}
	return strings.TrimSpace(strings.TrimSpace(contract.Scalar(item["fname"])) + " " + strings.TrimSpace(contract.Scalar(item["lname"])))
}

// visibility folds vis_state onto a status: active, deleted or archived.
func visibility(item record) string {
	switch contract.Scalar(item["vis_state"]) {
	case "0":
		return "active"
	case "1":
		return "deleted"
	case "2":
		return "archived"
	default:
		return ""
	}
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// newestUpdated is the change mark: the latest updated in a list as
// FreshBooks writes it, so the updated_since filter reads it back, or the
// previous mark when nothing newer appears.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(timeLayout, contract.Scalar(item["updated"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(timeLayout, previous); err == nil && before.After(newest) {
		return previous
	}
	return newest.Format(timeLayout)
}
