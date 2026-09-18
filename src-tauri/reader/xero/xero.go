// Package xero reads one Xero organisation behind the reader contract:
// three objects, contacts, invoices and payments, each flattened to text
// fields a mapping can point at. It uses the Accounting API over net/http
// alone, exchanges the app's refresh token for an access token once per
// process, names the tenant on every call, and writes nothing back.
// Backfill walks numbered pages, and Delta is one query for the newest
// change after the cursor's mark through the If-Modified-Since header.
package xero

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

// DefaultBaseURL is the production API host. A test points the reader
// elsewhere, and the token endpoint then follows the same host.
const DefaultBaseURL = "https://api.xero.com"

// DefaultTokenURL is Xero's token endpoint on the live identity service.
const DefaultTokenURL = "https://identity.xero.com/connect/token"

// tokenPath is where the token endpoint sits under an overridden base URL.
const tokenPath = "/connect/token"

// apiPath is where the Accounting API sits under the host.
const apiPath = "/api.xro/2.0"

// PageLimit is the most records one page returns.
const PageLimit = 1000

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// sinceLayout is the form the If-Modified-Since header reads, in UTC.
const sinceLayout = "2006-01-02T15:04:05"

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

// Source is one Xero tenant reached through one app.
type Source struct {
	baseURL      string
	tokenURL     string
	clientID     string
	clientSecret string
	refreshToken string
	tenantID     string
	client       *http.Client
	token        string
}

// New opens a source on the credentials the panel packs: the app's client
// id, client secret and refresh token, and the tenant id that names the
// organisation. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	tokenURL := DefaultTokenURL
	if baseURL == "" {
		baseURL = DefaultBaseURL
	} else {
		tokenURL = strings.TrimRight(baseURL, "/") + tokenPath
	}
	return &Source{
		baseURL:      strings.TrimRight(baseURL, "/"),
		tokenURL:     tokenURL,
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		refreshToken: credentials["refresh_token"],
		tenantID:     credentials["tenant_id"],
		client:       &http.Client{Timeout: 60 * time.Second},
	}
}

type record = map[string]any

// object is one endpoint the API reads and how each field reads.
type object struct {
	name   string
	label  string
	entity string
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "contacts", label: "Contacts", entity: "Contacts",
		fields: []field{
			{"id", "id", text("ContactID")},
			{"name", "string", text("Name")},
			{"first_name", "string", text("FirstName")},
			{"last_name", "string", text("LastName")},
			{"email", "email", text("EmailAddress")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", phone("DEFAULT")},
			{"mobile", "phone", phone("MOBILE")},
			{"website", "string", text("Website")},
			{"domain", "domain", websiteDomain},
			{"status", "string", lowered("ContactStatus")},
			{"is_customer", "boolean", boolean("IsCustomer")},
			{"is_supplier", "boolean", boolean("IsSupplier")},
			{"account_number", "string", text("AccountNumber")},
			{"tax_number", "string", text("TaxNumber")},
			{"outstanding", "number", balance("AccountsReceivable", "Outstanding")},
			{"overdue", "number", balance("AccountsReceivable", "Overdue")},
			{"currency", "string", text("DefaultCurrency")},
			{"street", "string", address("AddressLine1")},
			{"city", "string", address("City")},
			{"state", "string", address("Region")},
			{"postal_code", "string", address("PostalCode")},
			{"country", "string", address("Country")},
			{"modified", "date-time", when("UpdatedDateUTC")},
		},
	},
	{
		name: "invoices", label: "Invoices", entity: "Invoices",
		fields: []field{
			{"id", "id", text("InvoiceID")},
			{"number", "string", text("InvoiceNumber")},
			{"type", "string", invoiceType},
			{"contact_id", "id", nested("Contact", "ContactID")},
			{"contact_name", "string", nested("Contact", "Name")},
			{"status", "string", lowered("Status")},
			{"total", "number", amount("Total")},
			{"sub_total", "number", amount("SubTotal")},
			{"total_tax", "number", amount("TotalTax")},
			{"amount_due", "number", amount("AmountDue")},
			{"amount_paid", "number", amount("AmountPaid")},
			{"currency", "string", text("CurrencyCode")},
			{"date", "date", day("Date", "DateString")},
			{"due_date", "date", day("DueDate", "DueDateString")},
			{"fully_paid_date", "date", day("FullyPaidOnDate", "")},
			{"reference", "string", text("Reference")},
			{"url", "string", text("Url")},
			{"modified", "date-time", when("UpdatedDateUTC")},
		},
	},
	{
		name: "payments", label: "Payments", entity: "Payments",
		fields: []field{
			{"id", "id", text("PaymentID")},
			{"invoice_id", "id", nested("Invoice", "InvoiceID")},
			{"invoice_number", "string", nested("Invoice", "InvoiceNumber")},
			{"contact_id", "id", invoiceContact("ContactID")},
			{"contact_name", "string", invoiceContact("Name")},
			{"account_id", "id", nested("Account", "AccountID")},
			{"account_code", "string", nested("Account", "Code")},
			{"type", "string", lowered("PaymentType")},
			{"status", "string", lowered("Status")},
			{"amount", "number", amount("Amount")},
			{"currency", "string", nested("Invoice", "CurrencyCode")},
			{"currency_rate", "number", text("CurrencyRate")},
			{"date", "date", day("Date", "")},
			{"reference", "string", text("Reference")},
			{"reconciled", "boolean", boolean("IsReconciled")},
			{"modified", "date-time", when("UpdatedDateUTC")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Xero has no %s to read. The objects are contacts, invoices and payments.", name)}
}

// Objects signs in, proves the tenant with one small page, and lists the
// three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if err := s.signIn(); err != nil {
		return nil, err
	}
	if _, err := s.list(&objects[0], 1, 1, "", ""); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Xero counts nothing, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, err := s.list(object, 1, sampleRows, "", "")
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
	return &Description{Source: "xero", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: len(rows), Hash: newestModified(items, ""), Counted: false}, nil
}

// Page reads one numbered page, the cursor's token or the first. A full
// page means another follows. Total is the rows seen so far.
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
		if number, err := strconv.Atoi(cursor.Token); err == nil && number > 0 {
			page = number
		}
	}
	items, err := s.list(object, page, limit, "", "")
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestModified(items, hash)
	answer := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if len(items) == limit {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(page + 1)}
	}
	return answer, nil
}

// Delta asks for the one record changed after the mark, newest first, with
// the mark in the If-Modified-Since header.
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
	items, err := s.list(object, 1, 1, "UpdatedDateUTC DESC", mark)
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

// list reads one page of an endpoint in the order asked, every record when
// the order is empty, and only records changed after since when it is set.
func (s *Source) list(object *object, page, size int, order, since string) ([]record, error) {
	query := url.Values{"page": {strconv.Itoa(page)}, "pageSize": {strconv.Itoa(size)}}
	if order != "" {
		query.Set("order", order)
	}
	target := fmt.Sprintf("%s%s/%s?%s", s.baseURL, apiPath, object.entity, query.Encode())
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("xero-tenant-id", s.tenantID)
	if parsed, ok := parseTime(since); ok {
		request.Header.Set("If-Modified-Since", parsed.UTC().Format(sinceLayout))
	}
	body, status, err := s.do(request)
	if err != nil {
		return nil, err
	}
	if err := refusal(status, body); err != nil {
		return nil, err
	}
	var answer map[string]json.RawMessage
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Xero's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := answer[object.entity]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, &Failure{Code: "source", Message: fmt.Sprintf("Xero's %s do not parse: %s.", object.name, err)}
		}
	}
	return items, nil
}

// signIn exchanges the app's refresh token for an access token once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" || s.refreshToken == "" || s.tenantID == "" {
		return &Failure{Code: "not_connected", Message: "Connect Xero with its client id, client secret, refresh token and tenant id first."}
	}
	form := url.Values{"grant_type": {"refresh_token"}, "refresh_token": {s.refreshToken}}
	request, err := http.NewRequest(http.MethodPost, s.tokenURL, strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.SetBasicAuth(s.clientID, s.clientSecret)
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Xero refused the app: %s. Connect Xero again with a refresh token that is still valid.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Xero answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Xero's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Xero did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Xero's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Xero answers into the one failure the runtime
// reads. A 403 is a tenant the app is not connected to or a scope it lacks.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Xero refused the access token. Connect Xero again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Xero refused the request: %s. The app needs the accounting read scopes and a connection to this tenant.", faultMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Xero asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Xero answered %d: %s", status, faultMessage(body))}
	}
	return nil
}

// faultMessage reads the message out of either failure shape Xero writes:
// the problem document with its title and detail, or the API error with
// its message.
func faultMessage(body []byte) string {
	var failure struct {
		Title   string `json:"Title"`
		Detail  string `json:"Detail"`
		Message string `json:"Message"`
	}
	if json.Unmarshal(body, &failure) == nil {
		switch {
		case failure.Title != "" && failure.Detail != "":
			return failure.Title + ", " + failure.Detail
		case failure.Detail != "":
			return failure.Detail
		case failure.Title != "":
			return failure.Title
		case failure.Message != "":
			return failure.Message
		}
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

// day reads a date: the DateString Xero writes beside the date when the
// answer carries one, else the date itself as its calendar day in UTC.
func day(key, stringKey string) func(record) string {
	return func(item record) string {
		if stringKey != "" {
			if value := contract.Scalar(item[stringKey]); len(value) >= 10 {
				return value[:10]
			}
		}
		if parsed, ok := parseTime(contract.Scalar(item[key])); ok {
			return parsed.UTC().Format("2006-01-02")
		}
		return ""
	}
}

// amount reads a number Xero writes in the major unit as a decimal with
// two places.
func amount(key string) func(record) string {
	return func(item record) string { return formatAmount(item[key]) }
}

func formatAmount(value any) string {
	number, ok := value.(float64)
	if !ok {
		return ""
	}
	return strconv.FormatFloat(number, 'f', 2, 64)
}

// balance reads one figure under the contact's balances.
func balance(ledger, key string) func(record) string {
	return func(item record) string {
		balances, ok := item["Balances"].(map[string]any)
		if !ok {
			return ""
		}
		side, ok := balances[ledger].(map[string]any)
		if !ok {
			return ""
		}
		return formatAmount(side[key])
	}
}

// phone reads the number of one phone type, with its country and area codes
// in front when Xero writes them.
func phone(kind string) func(record) string {
	return func(item record) string {
		phones, _ := item["Phones"].([]any)
		for _, entry := range phones {
			line, _ := entry.(map[string]any)
			if contract.Scalar(line["PhoneType"]) != kind {
				continue
			}
			number := contract.Scalar(line["PhoneNumber"])
			if number == "" {
				return ""
			}
			parts := []string{}
			if country := contract.Scalar(line["PhoneCountryCode"]); country != "" {
				parts = append(parts, "+"+strings.TrimPrefix(country, "+"))
			}
			if area := contract.Scalar(line["PhoneAreaCode"]); area != "" {
				parts = append(parts, area)
			}
			return strings.Join(append(parts, number), " ")
		}
		return ""
	}
}

// address reads one field of the contact's street address, or its postal
// address when the street one is empty.
func address(key string) func(record) string {
	return func(item record) string {
		addresses, _ := item["Addresses"].([]any)
		for _, kind := range []string{"STREET", "POBOX"} {
			for _, entry := range addresses {
				line, _ := entry.(map[string]any)
				if contract.Scalar(line["AddressType"]) == kind {
					if value := contract.Scalar(line[key]); value != "" {
						return value
					}
				}
			}
		}
		return ""
	}
}

// invoiceType folds Xero's two invoice types onto words: a sales invoice
// is receivable, a bill is payable.
func invoiceType(item record) string {
	switch contract.Scalar(item["Type"]) {
	case "ACCREC":
		return "receivable"
	case "ACCPAY":
		return "payable"
	default:
		return strings.ToLower(contract.Scalar(item["Type"]))
	}
}

// invoiceContact reads one field of the contact under a payment's invoice.
func invoiceContact(key string) func(record) string {
	return func(item record) string {
		invoice, ok := item["Invoice"].(map[string]any)
		if !ok {
			return ""
		}
		if contact, ok := invoice["Contact"].(map[string]any); ok {
			return contract.Scalar(contact[key])
		}
		return ""
	}
}

func emailDomain(item record) string {
	email := contract.Scalar(item["EmailAddress"])
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
	return strings.TrimPrefix(strings.ToLower(parsed.Hostname()), "www.")
}

var dotNetDate = regexp.MustCompile(`^/Date\((-?\d+)([+-]\d{4})?\)/$`)

// parseTime reads the two forms Xero writes: the `/Date(1573755038357+0000)/`
// stamp in milliseconds since the epoch, and an ISO 8601 time with or
// without an offset.
func parseTime(value string) (time.Time, bool) {
	value = strings.TrimSpace(strings.ReplaceAll(value, `\/`, "/"))
	if value == "" {
		return time.Time{}, false
	}
	if match := dotNetDate.FindStringSubmatch(value); match != nil {
		millis, err := strconv.ParseInt(match[1], 10, 64)
		if err != nil {
			return time.Time{}, false
		}
		return time.UnixMilli(millis).UTC(), true
	}
	for _, layout := range []string{time.RFC3339Nano, "2006-01-02T15:04:05.999999999", "2006-01-02T15:04:05", "2006-01-02"} {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed, true
		}
	}
	return time.Time{}, false
}

// newestModified is the change mark: the latest UpdatedDateUTC in a list
// as RFC 3339 in UTC, or the previous mark when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, ok := parseTime(contract.Scalar(item["UpdatedDateUTC"])); ok && parsed.After(newest) {
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
