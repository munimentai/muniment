// Package paypal reads a PayPal business account behind the reader
// contract: two objects, transactions from the Reporting API and invoices
// from the Invoicing API, each flattened to text fields a mapping can point
// at. It uses the live REST API over net/http alone, signs in once per
// process by exchanging the app's client id and secret for an access token,
// and writes nothing back. Backfill walks transactions newest first in
// windows of thirty-one days, the widest range PayPal reports, and walks
// invoices by page number. Neither list filters by change time, so Delta
// reads the newest page and compares its latest update with the cursor's
// mark.
package paypal

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

// DefaultBaseURL is PayPal's live API host. A test points the reader
// elsewhere. The sandbox host is never offered.
const DefaultBaseURL = "https://api-m.paypal.com"

// TransactionPageLimit is the most transactions one report page returns.
const TransactionPageLimit = 500

// InvoicePageLimit is the most invoices one list page returns.
const InvoicePageLimit = 100

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// windowDays is the widest date range one transaction report covers.
const windowDays = 31

// historyYears is how far back PayPal reports transactions.
const historyYears = 3

// stamp is the date form the Reporting API reads and writes.
const stamp = "2006-01-02T15:04:05-0700"

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

// Source is one PayPal account reached through one REST app.
type Source struct {
	baseURL      string
	clientID     string
	clientSecret string
	client       *http.Client
	token        string
	lastRetry    string
	now          func() time.Time
}

// New opens a source on the credentials the panel packs: the app's client
// id and client secret. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	credentials := contract.Credentials(secret)
	return &Source{
		baseURL:      strings.TrimRight(baseURL, "/"),
		clientID:     credentials["client_id"],
		clientSecret: credentials["client_secret"],
		client:       &http.Client{Timeout: 60 * time.Second},
		now:          time.Now,
	}
}

type record = map[string]any

type object struct {
	name   string
	label  string
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "transactions", label: "Transactions",
		fields: []field{
			{"id", "id", info("transaction_id")},
			{"event_code", "string", info("transaction_event_code")},
			{"status", "string", transactionStatus},
			{"amount", "number", infoMoney("transaction_amount")},
			{"fee", "number", infoMoney("fee_amount")},
			{"currency", "string", infoCurrency("transaction_amount")},
			{"subject", "string", info("transaction_subject")},
			{"note", "string", info("transaction_note")},
			{"invoice_id", "id", info("invoice_id")},
			{"reference_id", "id", info("paypal_reference_id")},
			{"reference_type", "string", lowered(info("paypal_reference_id_type"))},
			{"custom_field", "string", info("custom_field")},
			{"payer_id", "id", payer("account_id")},
			{"payer_email", "email", payer("email_address")},
			{"payer_email_domain", "domain", emailDomain(payer("email_address"))},
			{"payer_name", "string", payerName},
			{"payer_country", "string", payer("country_code")},
			{"created", "date-time", when(info("transaction_initiation_date"))},
			{"modified", "date-time", when(info("transaction_updated_date"))},
		},
	},
	{
		name: "invoices", label: "Invoices",
		fields: []field{
			{"id", "id", text("id")},
			{"invoice_number", "string", detail("invoice_number")},
			{"status", "string", lowered(text("status"))},
			{"reference", "string", detail("reference")},
			{"total", "number", value("amount")},
			{"due", "number", value("due_amount")},
			{"paid", "number", paidAmount},
			{"currency", "string", invoiceCurrency},
			{"invoice_date", "date", detail("invoice_date")},
			{"due_date", "date", paymentTerm("due_date")},
			{"payment_term", "string", paymentTerm("term_type")},
			{"customer_name", "string", recipientName},
			{"customer_business", "string", recipient("business_name")},
			{"customer_email", "email", recipient("email_address")},
			{"customer_email_domain", "domain", emailDomain(recipient("email_address"))},
			{"note", "string", detail("note")},
			{"memo", "string", detail("memo")},
			{"recipient_view_url", "string", metadata("recipient_view_url")},
			{"created", "date-time", when(metadata("create_time"))},
			{"modified", "date-time", when(metadata("last_update_time"))},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("PayPal has no %s to read. The objects are transactions and invoices.", name)}
}

// Objects signs in, which proves the app's credentials, and lists the two.
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
// The invoice list names its total, so Counted is true there. Transactions
// arrive by window, so Rows is the rows read and Counted is false.
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
	total := page.Total
	if !page.Counted {
		total = len(page.Rows)
	}
	return &Description{
		Source:  "paypal",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, page.Rows),
		Rows:    total,
		Bytes:   0,
		Hash:    page.Hash,
		Counted: page.Counted,
	}, nil
}

// Page reads one page after the cursor's token. A transaction token names
// the end of the window and the page within it. An invoice token is the
// page number.
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
	total := 0
	counted := false
	if object.name == "transactions" {
		items, next, err = s.transactionPage(token, limit)
	} else {
		items, next, total, err = s.invoicePage(token, limit)
		counted = true
	}
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestUpdated(object, items, hash)
	if !counted {
		total = offset + len(rows)
	}
	page := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: counted}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta reads the newest page and compares its latest update with the
// mark. PayPal filters neither list by change time, so a changed record
// outside the newest page reads as unchanged here, and the run lands it
// anyway.
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
	var items []record
	if object.name == "transactions" {
		end := s.now().UTC()
		items, _, err = s.transactions(end.AddDate(0, 0, -windowDays), end, 1, TransactionPageLimit)
	} else {
		items, _, _, err = s.invoicePage("", InvoicePageLimit)
	}
	if err != nil {
		return nil, err
	}
	newest := newestUpdated(object, items, mark)
	if newest == mark && mark != "" {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// transactionPage walks the report newest first: the pages of one window,
// then the window before it, until the history PayPal keeps runs out. A
// window with nothing in it is skipped within the call, so a page answers
// rows or the end.
func (s *Source) transactionPage(token string, limit int) ([]record, string, error) {
	if limit <= 0 || limit > TransactionPageLimit {
		limit = TransactionPageLimit
	}
	now := s.now().UTC()
	floor := now.AddDate(-historyYears, 0, 1)
	end, page := now, 1
	if token != "" {
		parsedEnd, parsedPage, ok := parseToken(token)
		if !ok {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("The transaction cursor %q does not parse.", token)}
		}
		end, page = parsedEnd, parsedPage
	}
	for {
		start := end.AddDate(0, 0, -windowDays)
		if start.Before(floor) {
			start = floor
		}
		items, totalPages, err := s.transactions(start, end, page, limit)
		if err != nil {
			return nil, "", err
		}
		switch {
		case page < totalPages:
			return items, formatToken(end, page+1), nil
		case !start.After(floor):
			return items, "", nil
		case len(items) > 0:
			return items, formatToken(start, 1), nil
		}
		end, page = start, 1
	}
}

func formatToken(end time.Time, page int) string {
	return end.UTC().Format(time.RFC3339) + "|" + strconv.Itoa(page)
}

func parseToken(token string) (time.Time, int, bool) {
	parts := strings.SplitN(token, "|", 2)
	if len(parts) != 2 {
		return time.Time{}, 0, false
	}
	end, err := time.Parse(time.RFC3339, parts[0])
	if err != nil {
		return time.Time{}, 0, false
	}
	page, err := strconv.Atoi(parts[1])
	if err != nil || page < 1 {
		return time.Time{}, 0, false
	}
	return end, page, true
}

// transactions reads one page of the report for one window and answers
// its records and the window's page count.
func (s *Source) transactions(start, end time.Time, page, limit int) ([]record, int, error) {
	query := url.Values{
		"start_date": {start.UTC().Format(stamp)},
		"end_date":   {end.UTC().Format(stamp)},
		"fields":     {"transaction_info,payer_info"},
		"page_size":  {strconv.Itoa(limit)},
		"page":       {strconv.Itoa(page)},
	}
	body, status, err := s.get("/v1/reporting/transactions", query)
	if err != nil {
		return nil, 0, err
	}
	if err := s.refusal("transactions", status, body); err != nil {
		return nil, 0, err
	}
	var answer struct {
		Details    []record `json:"transaction_details"`
		TotalPages int      `json:"total_pages"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, 0, &Failure{Code: "source", Message: fmt.Sprintf("PayPal's answer does not parse: %s.", err)}
	}
	return answer.Details, answer.TotalPages, nil
}

// invoicePage reads one page of the invoice list and answers its records,
// the next page number and the list's total.
func (s *Source) invoicePage(token string, limit int) ([]record, string, int, error) {
	if limit <= 0 || limit > InvoicePageLimit {
		limit = InvoicePageLimit
	}
	page := 1
	if token != "" {
		parsed, err := strconv.Atoi(token)
		if err != nil || parsed < 1 {
			return nil, "", 0, &Failure{Code: "source", Message: fmt.Sprintf("The invoice cursor %q does not parse.", token)}
		}
		page = parsed
	}
	query := url.Values{"page": {strconv.Itoa(page)}, "page_size": {strconv.Itoa(limit)}, "total_required": {"true"}}
	body, status, err := s.get("/v2/invoicing/invoices", query)
	if err != nil {
		return nil, "", 0, err
	}
	if err := s.refusal("invoices", status, body); err != nil {
		return nil, "", 0, err
	}
	var answer struct {
		Items      []record `json:"items"`
		TotalItems int      `json:"total_items"`
		TotalPages int      `json:"total_pages"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, "", 0, &Failure{Code: "source", Message: fmt.Sprintf("PayPal's answer does not parse: %s.", err)}
	}
	next := ""
	if page < answer.TotalPages {
		next = strconv.Itoa(page + 1)
	}
	return answer.Items, next, answer.TotalItems, nil
}

// signIn exchanges the app's client id and secret for an access token once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" {
		return &Failure{Code: "not_connected", Message: "Connect PayPal with its client id and client secret first."}
	}
	request, err := http.NewRequest(http.MethodPost, s.baseURL+"/v1/oauth2/token", strings.NewReader("grant_type=client_credentials"))
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
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("PayPal refused the client id and secret: %s. Connect PayPal again with the live app's credentials.", paypalMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("PayPal answered %d at sign-in: %s", status, paypalMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "PayPal's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
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
	request.Header.Set("Authorization", "Bearer "+s.token)
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("PayPal did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("PayPal's answer did not read: %s.", err)}
	}
	s.lastRetry = response.Header.Get("Retry-After")
	return body, response.StatusCode, nil
}

// refusal turns a status PayPal answers into the one failure the runtime
// reads. A 403 names a feature the app lacks: Transaction Search for the
// report, Invoicing for the invoices.
func (s *Source) refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "PayPal refused the access token. Connect PayPal again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("PayPal refused the request for %s: %s. The app needs Transaction Search and Invoicing enabled.", object, paypalMessage(body))}
	case status == http.StatusTooManyRequests:
		if seconds, err := strconv.Atoi(strings.TrimSpace(s.lastRetry)); err == nil && seconds > 0 {
			return &Failure{Code: "rate_limited", Message: fmt.Sprintf("PayPal asked the reader to wait %d seconds. Run again then.", seconds)}
		}
		return &Failure{Code: "rate_limited", Message: "PayPal asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("PayPal answered %d: %s", status, paypalMessage(body))}
	}
	return nil
}

// paypalMessage reads the message out of either failure shape PayPal
// writes: the OAuth error and description, or the named error with its
// message and first detail.
func paypalMessage(body []byte) string {
	var failure struct {
		Error       string `json:"error"`
		Description string `json:"error_description"`
		Name        string `json:"name"`
		Message     string `json:"message"`
		Details     []struct {
			Issue       string `json:"issue"`
			Description string `json:"description"`
		} `json:"details"`
	}
	if json.Unmarshal(body, &failure) == nil {
		switch {
		case failure.Error != "" && failure.Description != "":
			return failure.Error + ", " + failure.Description
		case failure.Error != "":
			return failure.Error
		case len(failure.Details) > 0 && failure.Details[0].Description != "":
			return failure.Details[0].Description
		case failure.Message != "":
			return failure.Message
		case failure.Name != "":
			return failure.Name
		}
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

type reader = func(record) string

func text(key string) reader {
	return func(item record) string { return contract.Scalar(item[key]) }
}

func child(item record, keys ...string) map[string]any {
	current := item
	for _, key := range keys {
		next, ok := current[key].(map[string]any)
		if !ok {
			return nil
		}
		current = next
	}
	return current
}

func nested(read reader, parents ...string) reader {
	return func(item record) string {
		inner := child(item, parents...)
		if inner == nil {
			return ""
		}
		return read(inner)
	}
}

func lowered(read reader) reader {
	return func(item record) string { return strings.ToLower(read(item)) }
}

func when(read reader) reader {
	return func(item record) string { return formatTime(read(item)) }
}

func emailDomain(read reader) reader {
	return func(item record) string {
		email := read(item)
		if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
			return strings.ToLower(email[at+1:])
		}
		return ""
	}
}

// info reads one transaction_info field.
func info(key string) reader { return nested(text(key), "transaction_info") }

// payer reads one payer_info field.
func payer(key string) reader { return nested(text(key), "payer_info") }

// infoMoney reads a transaction money object's value, a decimal PayPal
// already writes in the major unit.
func infoMoney(key string) reader { return nested(text("value"), "transaction_info", key) }

func infoCurrency(key string) reader {
	return nested(text("currency_code"), "transaction_info", key)
}

// transactionStatus folds PayPal's one letter statuses onto words.
func transactionStatus(item record) string {
	switch info("transaction_status")(item) {
	case "S":
		return "completed"
	case "P":
		return "pending"
	case "D":
		return "denied"
	case "V":
		return "reversed"
	default:
		return ""
	}
}

// payerName reads the payer's full name, or the given name and surname
// when PayPal writes no full name.
func payerName(item record) string {
	name := child(item, "payer_info", "payer_name")
	if name == nil {
		return ""
	}
	if full := contract.Scalar(name["alternate_full_name"]); full != "" {
		return full
	}
	return strings.TrimSpace(contract.Scalar(name["given_name"]) + " " + contract.Scalar(name["surname"]))
}

// detail reads one invoice detail field.
func detail(key string) reader { return nested(text(key), "detail") }

func metadata(key string) reader { return nested(text(key), "detail", "metadata") }

func paymentTerm(key string) reader { return nested(text(key), "detail", "payment_term") }

// value reads an invoice money object's value.
func value(key string) reader { return nested(text("value"), key) }

func paidAmount(item record) string { return nested(text("value"), "payments", "paid_amount")(item) }

func invoiceCurrency(item record) string {
	if currency := nested(text("currency_code"), "amount")(item); currency != "" {
		return currency
	}
	return detail("currency_code")(item)
}

func primaryRecipient(item record) map[string]any {
	list, ok := item["primary_recipients"].([]any)
	if !ok || len(list) == 0 {
		return nil
	}
	first, _ := list[0].(map[string]any)
	return child(first, "billing_info")
}

// recipient reads one billing field of the invoice's first recipient.
func recipient(key string) reader {
	return func(item record) string {
		billing := primaryRecipient(item)
		if billing == nil {
			return ""
		}
		return contract.Scalar(billing[key])
	}
}

func recipientName(item record) string {
	billing := primaryRecipient(item)
	if billing == nil {
		return ""
	}
	name := child(billing, "name")
	if name == nil {
		return ""
	}
	if full := contract.Scalar(name["full_name"]); full != "" {
		return full
	}
	return strings.TrimSpace(contract.Scalar(name["given_name"]) + " " + contract.Scalar(name["surname"]))
}

// formatTime reads PayPal's `2023-11-14T22:13:20+0000` and RFC 3339 stamps
// as RFC 3339 in UTC.
func formatTime(value string) string {
	if parsed, ok := parseTime(value); ok {
		return parsed.UTC().Format(time.RFC3339)
	}
	return strings.TrimSpace(value)
}

func parseTime(value string) (time.Time, bool) {
	value = strings.TrimSpace(value)
	if value == "" {
		return time.Time{}, false
	}
	for _, layout := range []string{stamp, time.RFC3339Nano} {
		if parsed, err := time.Parse(layout, value); err == nil {
			return parsed, true
		}
	}
	return time.Time{}, false
}

// newestUpdated is the change mark: the latest update in a list as RFC
// 3339 in UTC, or the previous mark when nothing newer appears. A
// transaction's update is transaction_updated_date, an invoice's is its
// metadata's last_update_time.
func newestUpdated(object *object, items []record, previous string) string {
	read := info("transaction_updated_date")
	if object.name == "invoices" {
		read = metadata("last_update_time")
	}
	var newest time.Time
	for _, item := range items {
		if parsed, ok := parseTime(read(item)); ok && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, ok := parseTime(previous); ok && !newest.After(before) {
		return previous
	}
	return newest.UTC().Format(time.RFC3339)
}
