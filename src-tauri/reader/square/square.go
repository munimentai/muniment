// Package square reads a Square account behind the reader contract: four
// objects, customers, orders, payments and invoices, each flattened to text
// fields a mapping can point at. It uses the Connect v2 REST API over
// net/http alone, with a personal access token as a bearer token, and it
// writes nothing back. Backfill walks each list through Square's own
// cursor, and Delta is one query for the newest change after the cursor's
// mark where Square filters by update time.
package square

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

// DefaultBaseURL is Square's production API host. A test points the
// reader elsewhere.
const DefaultBaseURL = "https://connect.squareup.com"

// APIVersion is the Square-Version header every request names.
const APIVersion = "2024-06-04"

// PageLimit is the most records one list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
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

// Source is one Square account.
type Source struct {
	token     string
	baseURL   string
	client    *http.Client
	locations []string
	located   bool
	lastRetry string
}

// New opens a source on a personal access token. An empty base URL is the
// production API.
func New(secret, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:   contract.Credentials(secret)["token"],
		baseURL: strings.TrimRight(baseURL, "/"),
		client:  &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one Square list and the fields its rows carry, in the order
// Describe lists them. The key names the list in Square's answer.
type object struct {
	name   string
	label  string
	key    string
	fields []field
}

// field names one text column and how it reads out of a Square record.
type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "customers", label: "Customers", key: "customers",
		fields: []field{
			{"id", "id", text("id")},
			{"given_name", "string", text("given_name")},
			{"family_name", "string", text("family_name")},
			{"name", "string", fullName("given_name", "family_name")},
			{"company_name", "string", text("company_name")},
			{"email", "email", text("email_address")},
			{"email_domain", "domain", emailDomain("email_address")},
			{"phone", "phone", text("phone_number")},
			{"address_line1", "string", nested("address", "address_line_1")},
			{"address_city", "string", nested("address", "locality")},
			{"address_state", "string", nested("address", "administrative_district_level_1")},
			{"address_postal_code", "string", nested("address", "postal_code")},
			{"address_country", "string", nested("address", "country")},
			{"reference_id", "string", text("reference_id")},
			{"note", "string", text("note")},
			{"creation_source", "string", lowered("creation_source")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "orders", label: "Orders", key: "orders",
		fields: []field{
			{"id", "id", text("id")},
			{"location_id", "id", text("location_id")},
			{"customer_id", "id", text("customer_id")},
			{"state", "string", text("state")},
			{"status", "string", orderStatus},
			{"reference_id", "string", text("reference_id")},
			{"source", "string", nested("source", "name")},
			{"total", "number", money("total_money")},
			{"tax", "number", money("total_tax_money")},
			{"discount", "number", money("total_discount_money")},
			{"tip", "number", money("total_tip_money")},
			{"amount_due", "number", money("net_amount_due_money")},
			{"currency", "string", currency("total_money")},
			{"line_items", "number", count("line_items")},
			{"created", "date-time", when("created_at")},
			{"closed_at", "date-time", when("closed_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "payments", label: "Payments", key: "payments",
		fields: []field{
			{"id", "id", text("id")},
			{"order_id", "id", text("order_id")},
			{"customer_id", "id", text("customer_id")},
			{"location_id", "id", text("location_id")},
			{"status", "string", lowered("status")},
			{"amount", "number", money("amount_money")},
			{"tip", "number", money("tip_money")},
			{"total", "number", money("total_money")},
			{"refunded", "number", money("refunded_money")},
			{"currency", "string", currency("amount_money")},
			{"source_type", "string", lowered("source_type")},
			{"card_brand", "string", cardDetail("card_brand")},
			{"card_last_4", "string", cardDetail("last_4")},
			{"receipt_number", "string", text("receipt_number")},
			{"receipt_url", "string", text("receipt_url")},
			{"reference_id", "string", text("reference_id")},
			{"note", "string", text("note")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "invoices", label: "Invoices", key: "invoices",
		fields: []field{
			{"id", "id", text("id")},
			{"invoice_number", "string", text("invoice_number")},
			{"title", "string", text("title")},
			{"status", "string", lowered("status")},
			{"location_id", "id", text("location_id")},
			{"order_id", "id", text("order_id")},
			{"customer_id", "id", nested("primary_recipient", "customer_id")},
			{"customer_name", "string", recipientName},
			{"customer_email", "email", nested("primary_recipient", "email_address")},
			{"total", "number", requestedAmount("computed_amount_money")},
			{"paid", "number", requestedAmount("total_completed_amount_money")},
			{"currency", "string", requestedCurrency},
			{"due_date", "date", lastDueDate},
			{"public_url", "string", text("public_url")},
			{"scheduled_at", "date-time", when("scheduled_at")},
			{"sale_date", "date", text("sale_or_service_date")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Square has no %s to read. The objects are customers, orders, payments and invoices.", name)}
}

// Objects proves the token by listing the account's locations, which the
// order and invoice searches need anyway, and lists the four.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.token == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Square with its access token first."}
	}
	if _, err := s.locationIDs(); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Square counts nothing, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	items, _, err := s.list(object, "", sampleRows)
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
	return &Description{
		Source:  "square",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestUpdated(items, ""),
		Counted: false,
	}, nil
}

// Page reads one page after the cursor's token. Total is the rows seen so
// far, because Square counts nothing.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, token := 0, "", ""
	if cursor != nil {
		offset, hash, token = cursor.Offset, cursor.Hash, cursor.Token
	}
	items, next, err := s.list(object, token, limit)
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

// Delta asks for the records changed after the mark. Customers, orders
// and payments filter by update time. Invoices carry no such filter, so
// their first page stands in, and a changed old invoice reads as unchanged
// here until the run lands it.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	items, err := s.changed(object, mark)
	if err != nil {
		return nil, err
	}
	newest := newestUpdated(items, mark)
	if newest == mark && mark != "" {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// list fetches one page of an object after the cursor and answers its
// records and the next cursor. Orders and invoices are searches over every
// location, because Square lists neither without one.
func (s *Source) list(object *object, cursor string, limit int) ([]record, string, error) {
	switch object.name {
	case "customers":
		query := url.Values{"limit": {strconv.Itoa(limit)}}
		if cursor != "" {
			query.Set("cursor", cursor)
		}
		return s.page(object, http.MethodGet, "/v2/customers", query, nil)
	case "payments":
		query := url.Values{"limit": {strconv.Itoa(limit)}}
		if cursor != "" {
			query.Set("cursor", cursor)
		}
		return s.page(object, http.MethodGet, "/v2/payments", query, nil)
	case "orders":
		locations, err := s.locationIDs()
		if err != nil || len(locations) == 0 {
			return nil, "", err
		}
		body := map[string]any{"location_ids": locations, "limit": limit}
		if cursor != "" {
			body["cursor"] = cursor
		}
		return s.page(object, http.MethodPost, "/v2/orders/search", nil, body)
	default:
		locations, err := s.locationIDs()
		if err != nil || len(locations) == 0 {
			return nil, "", err
		}
		body := map[string]any{
			"query": map[string]any{
				"filter": map[string]any{"location_ids": locations},
				"sort":   map[string]any{"field": "INVOICE_SORT_DATE", "order": "DESC"},
			},
			"limit": limit,
		}
		if cursor != "" {
			body["cursor"] = cursor
		}
		return s.page(object, http.MethodPost, "/v2/invoices/search", nil, body)
	}
}

// changed fetches the records updated after the mark, or the newest page
// when the mark is empty or the object carries no update filter.
func (s *Source) changed(object *object, mark string) ([]record, error) {
	var items []record
	var err error
	switch object.name {
	case "customers":
		body := map[string]any{"limit": PageLimit}
		if mark != "" {
			body["query"] = map[string]any{"filter": map[string]any{"updated_at": map[string]any{"start_at": mark}}}
		}
		items, _, err = s.page(object, http.MethodPost, "/v2/customers/search", nil, body)
	case "payments":
		query := url.Values{"limit": {"1"}, "sort_field": {"UPDATED_AT"}, "sort_order": {"DESC"}}
		if mark != "" {
			query.Set("updated_at_begin_time", mark)
		}
		items, _, err = s.page(object, http.MethodGet, "/v2/payments", query, nil)
	case "orders":
		locations, failure := s.locationIDs()
		if failure != nil || len(locations) == 0 {
			return nil, failure
		}
		search := map[string]any{"sort": map[string]any{"sort_field": "UPDATED_AT", "sort_order": "DESC"}}
		if mark != "" {
			search["filter"] = map[string]any{"date_time_filter": map[string]any{"updated_at": map[string]any{"start_at": mark}}}
		}
		items, _, err = s.page(object, http.MethodPost, "/v2/orders/search", nil, map[string]any{"location_ids": locations, "query": search, "limit": 1})
	default:
		items, _, err = s.list(object, "", PageLimit)
	}
	return items, err
}

// locationIDs reads the account's locations once per source.
func (s *Source) locationIDs() ([]string, error) {
	if s.located {
		return s.locations, nil
	}
	body, status, err := s.do(http.MethodGet, "/v2/locations", nil, nil)
	if err != nil {
		return nil, err
	}
	if err := s.refusal("locations", status, body); err != nil {
		return nil, err
	}
	var listed struct {
		Locations []struct {
			ID string `json:"id"`
		} `json:"locations"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Square's answer does not parse: %s.", err)}
	}
	ids := make([]string, 0, len(listed.Locations))
	for _, location := range listed.Locations {
		if location.ID != "" {
			ids = append(ids, location.ID)
		}
	}
	s.locations = ids
	s.located = true
	return ids, nil
}

// page runs one list or search call and answers the records under the
// object's key and Square's cursor for the next page.
func (s *Source) page(object *object, method, path string, query url.Values, body map[string]any) ([]record, string, error) {
	answer, status, err := s.do(method, path, query, body)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(object.name, status, answer); err != nil {
		return nil, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(answer, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Square's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := listed[object.key]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Square's %s do not parse: %s.", object.key, err)}
		}
	}
	next := ""
	if raw, ok := listed["cursor"]; ok {
		_ = json.Unmarshal(raw, &next)
	}
	return items, next, nil
}

func (s *Source) do(method, path string, query url.Values, body map[string]any) ([]byte, int, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	var payload io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return nil, 0, &Failure{Code: "source", Message: err.Error()}
		}
		payload = bytes.NewReader(encoded)
	}
	request, err := http.NewRequest(method, target, payload)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Square-Version", APIVersion)
	request.Header.Set("Accept", "application/json")
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Square did not answer: %s.", err)}
	}
	defer response.Body.Close()
	answer, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Square's answer did not read: %s.", err)}
	}
	s.lastRetry = response.Header.Get("Retry-After")
	return answer, response.StatusCode, nil
}

// refusal turns a status Square answers into the one failure the runtime
// reads.
func (s *Source) refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Square refused the access token. Connect Square again with a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Square refused the request for %s: %s. The token needs read permission on them.", object, squareMessage(body))}
	case status == http.StatusTooManyRequests:
		if seconds, err := strconv.Atoi(strings.TrimSpace(s.lastRetry)); err == nil && seconds > 0 {
			return &Failure{Code: "rate_limited", Message: fmt.Sprintf("Square asked the reader to wait %d seconds. Run again then.", seconds)}
		}
		return &Failure{Code: "rate_limited", Message: "Square asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Square answered %d: %s", status, squareMessage(body))}
	}
	return nil
}

func squareMessage(body []byte) string {
	var failure struct {
		Errors []struct {
			Code   string `json:"code"`
			Detail string `json:"detail"`
		} `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Errors) > 0 {
		first := failure.Errors[0]
		if first.Detail != "" {
			return first.Detail
		}
		return first.Code
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

func count(key string) func(record) string {
	return func(item record) string {
		list, ok := item[key].([]any)
		if !ok {
			return ""
		}
		return strconv.Itoa(len(list))
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Square's RFC 3339 stamps as RFC 3339 in UTC.
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

func fullName(firstKey, lastKey string) func(record) string {
	return func(item record) string {
		return strings.TrimSpace(contract.Scalar(item[firstKey]) + " " + contract.Scalar(item[lastKey]))
	}
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

// money reads a Square money object as a decimal in the major unit. Square
// counts amounts in the smallest unit, and a zero decimal currency such as
// JPY keeps its whole number.
func money(key string) func(record) string {
	return func(item record) string {
		amount, ok := item[key].(map[string]any)
		if !ok {
			return ""
		}
		return formatMoney(amount)
	}
}

func currency(key string) func(record) string {
	return func(item record) string {
		amount, ok := item[key].(map[string]any)
		if !ok {
			return ""
		}
		return strings.ToUpper(contract.Scalar(amount["currency"]))
	}
}

var zeroDecimalCurrencies = map[string]bool{
	"BIF": true, "CLP": true, "DJF": true, "GNF": true, "JPY": true, "KMF": true, "KRW": true,
	"MGA": true, "PYG": true, "RWF": true, "UGX": true, "VND": true, "VUV": true, "XAF": true,
	"XOF": true, "XPF": true,
}

func formatMoney(amount map[string]any) string {
	minor, ok := amount["amount"].(float64)
	if !ok {
		return ""
	}
	return formatAmount(minor, contract.Scalar(amount["currency"]))
}

func formatAmount(minor float64, currency string) string {
	if zeroDecimalCurrencies[strings.ToUpper(currency)] {
		return strconv.FormatFloat(minor, 'f', 0, 64)
	}
	return strconv.FormatFloat(minor/100, 'f', 2, 64)
}

// orderStatus folds Square's order states onto lower case words, with the
// American spelling Square uses folded onto the one the kinds use.
func orderStatus(item record) string {
	switch contract.Scalar(item["state"]) {
	case "OPEN":
		return "open"
	case "COMPLETED":
		return "completed"
	case "CANCELED":
		return "cancelled"
	case "DRAFT":
		return "draft"
	default:
		return ""
	}
}

func cardDetail(key string) func(record) string {
	return func(item record) string {
		details, ok := item["card_details"].(map[string]any)
		if !ok {
			return ""
		}
		card, ok := details["card"].(map[string]any)
		if !ok {
			return ""
		}
		return contract.Scalar(card[key])
	}
}

// recipientName reads an invoice's recipient as a person's name, or the
// company when no person is named.
func recipientName(item record) string {
	recipient, ok := item["primary_recipient"].(map[string]any)
	if !ok {
		return ""
	}
	if name := fullName("given_name", "family_name")(recipient); name != "" {
		return name
	}
	return contract.Scalar(recipient["company_name"])
}

func paymentRequests(item record) []map[string]any {
	list, ok := item["payment_requests"].([]any)
	if !ok {
		return nil
	}
	requests := make([]map[string]any, 0, len(list))
	for _, entry := range list {
		if request, ok := entry.(map[string]any); ok {
			requests = append(requests, request)
		}
	}
	return requests
}

// requestedAmount sums one money field over an invoice's payment requests,
// so an invoice paid in parts reads as one total and one paid amount.
func requestedAmount(key string) func(record) string {
	return func(item record) string {
		requests := paymentRequests(item)
		var minor float64
		currency := ""
		found := false
		for _, request := range requests {
			amount, ok := request[key].(map[string]any)
			if !ok {
				continue
			}
			if value, ok := amount["amount"].(float64); ok {
				minor += value
				found = true
			}
			if currency == "" {
				currency = contract.Scalar(amount["currency"])
			}
		}
		if !found {
			return ""
		}
		return formatAmount(minor, currency)
	}
}

func requestedCurrency(item record) string {
	for _, request := range paymentRequests(item) {
		if amount, ok := request["computed_amount_money"].(map[string]any); ok {
			if currency := contract.Scalar(amount["currency"]); currency != "" {
				return strings.ToUpper(currency)
			}
		}
	}
	return ""
}

// lastDueDate is the latest due date across an invoice's payment requests.
func lastDueDate(item record) string {
	last := ""
	for _, request := range paymentRequests(item) {
		if due := contract.Scalar(request["due_date"]); due > last {
			last = due
		}
	}
	return last
}

// newestUpdated is the change mark: the latest updated_at in a list as
// RFC 3339 in UTC, so a filter reads it back, or the previous mark when
// nothing newer appears.
func newestUpdated(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["updated_at"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(time.RFC3339Nano, previous); err == nil && !newest.After(before) {
		return previous
	}
	return newest.UTC().Format(time.RFC3339)
}
