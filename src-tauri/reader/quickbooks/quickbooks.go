// Package quickbooks reads a QuickBooks Online company behind the reader
// contract: three objects, customers, invoices and payments, each flattened
// to text fields a mapping can point at. It uses the query endpoint of the
// v3 REST API over net/http alone, exchanges the app's refresh token for an
// access token once per process, and writes nothing back. Backfill walks a
// query through its start position, and Delta is one query for the newest
// change after the cursor's mark.
package quickbooks

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

// DefaultBaseURL is the production API host. A test points the reader
// elsewhere, and the token endpoint then follows the same host.
const DefaultBaseURL = "https://quickbooks.api.intuit.com"

// DefaultTokenURL is Intuit's token endpoint on the live platform.
const DefaultTokenURL = "https://oauth.platform.intuit.com/oauth2/v1/tokens/bearer"

// tokenPath is where the token endpoint sits under an overridden base URL.
const tokenPath = "/oauth2/v1/tokens/bearer"

// MinorVersion is the API minor version every query names.
const MinorVersion = "70"

// PageLimit is the most records one query returns.
const PageLimit = 1000

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

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

// Source is one QuickBooks Online company reached through one app.
type Source struct {
	baseURL      string
	tokenURL     string
	clientID     string
	clientSecret string
	refreshToken string
	realmID      string
	client       *http.Client
	token        string
}

// New opens a source on the credentials the panel packs: the app's client
// id, client secret and refresh token, and the realm id that names the
// company. An empty base URL is the live API.
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
		realmID:      credentials["realm_id"],
		client:       &http.Client{Timeout: 60 * time.Second},
	}
}

type record = map[string]any

// object is one entity the query endpoint reads and how each field reads.
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
		name: "customers", label: "Customers", entity: "Customer",
		fields: []field{
			{"id", "id", text("Id")},
			{"name", "string", text("DisplayName")},
			{"company", "string", text("CompanyName")},
			{"first_name", "string", text("GivenName")},
			{"last_name", "string", text("FamilyName")},
			{"email", "email", nested("PrimaryEmailAddr", "Address")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", nested("PrimaryPhone", "FreeFormNumber")},
			{"website", "string", nested("WebAddr", "URI")},
			{"domain", "domain", websiteDomain},
			{"balance", "number", amount("Balance")},
			{"currency", "string", nested("CurrencyRef", "value")},
			{"active", "boolean", boolean("Active")},
			{"taxable", "boolean", boolean("Taxable")},
			{"street", "string", nested("BillAddr", "Line1")},
			{"city", "string", nested("BillAddr", "City")},
			{"state", "string", nested("BillAddr", "CountrySubDivisionCode")},
			{"postal_code", "string", nested("BillAddr", "PostalCode")},
			{"country", "string", nested("BillAddr", "Country")},
			{"parent_id", "id", nested("ParentRef", "value")},
			{"notes", "string", text("Notes")},
			{"created", "date-time", nestedWhen("MetaData", "CreateTime")},
			{"modified", "date-time", nestedWhen("MetaData", "LastUpdatedTime")},
		},
	},
	{
		name: "invoices", label: "Invoices", entity: "Invoice",
		fields: []field{
			{"id", "id", text("Id")},
			{"number", "string", text("DocNumber")},
			{"customer_id", "id", nested("CustomerRef", "value")},
			{"customer_name", "string", nested("CustomerRef", "name")},
			{"customer_email", "email", nested("BillEmail", "Address")},
			{"status", "string", invoiceStatus},
			{"total", "number", amount("TotalAmt")},
			{"balance", "number", amount("Balance")},
			{"amount_paid", "number", amountPaid},
			{"currency", "string", nested("CurrencyRef", "value")},
			{"date", "date", text("TxnDate")},
			{"due_date", "date", text("DueDate")},
			{"email_status", "string", text("EmailStatus")},
			{"memo", "string", nested("CustomerMemo", "value")},
			{"note", "string", text("PrivateNote")},
			{"created", "date-time", nestedWhen("MetaData", "CreateTime")},
			{"modified", "date-time", nestedWhen("MetaData", "LastUpdatedTime")},
		},
	},
	{
		name: "payments", label: "Payments", entity: "Payment",
		fields: []field{
			{"id", "id", text("Id")},
			{"customer_id", "id", nested("CustomerRef", "value")},
			{"customer_name", "string", nested("CustomerRef", "name")},
			{"invoice_id", "id", linkedInvoice},
			{"total", "number", amount("TotalAmt")},
			{"unapplied", "number", amount("UnappliedAmt")},
			{"currency", "string", nested("CurrencyRef", "value")},
			{"date", "date", text("TxnDate")},
			{"reference", "string", text("PaymentRefNum")},
			{"method", "string", nested("PaymentMethodRef", "name")},
			{"deposit_account_id", "id", nested("DepositToAccountRef", "value")},
			{"note", "string", text("PrivateNote")},
			{"created", "date-time", nestedWhen("MetaData", "CreateTime")},
			{"modified", "date-time", nestedWhen("MetaData", "LastUpdatedTime")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("QuickBooks has no %s to read. The objects are customers, invoices and payments.", name)}
}

// Objects signs in, proves the realm with one small query, and lists the
// three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if err := s.signIn(); err != nil {
		return nil, err
	}
	if _, err := s.query("SELECT Id FROM Customer MAXRESULTS 1", "Customer"); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// A count query names the total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	items, err := s.query(object.select_("", 1, sampleRows), object.entity)
	if err != nil {
		return nil, err
	}
	total, err := s.count(object)
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
	return &Description{Source: "quickbooks", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: total, Hash: newestModified(items, ""), Counted: true}, nil
}

// Page reads one query page from the start position the cursor's token
// names, the first when it names none. A full page means another follows.
// Total is the rows seen so far, because a count is a second query.
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
	offset, hash, start := 0, "", 1
	if cursor != nil {
		offset, hash = cursor.Offset, cursor.Hash
		if position, err := strconv.Atoi(cursor.Token); err == nil && position > 0 {
			start = position
		}
	}
	items, err := s.query(object.select_("", start, limit), object.entity)
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
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(start + limit)}
	}
	return answer, nil
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
		where = fmt.Sprintf("WHERE MetaData.LastUpdatedTime > '%s'", mark)
	}
	soql := fmt.Sprintf("SELECT Id, MetaData FROM %s %s ORDERBY MetaData.LastUpdatedTime DESC MAXRESULTS 1", object.entity, where)
	items, err := s.query(strings.Join(strings.Fields(soql), " "), object.entity)
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

// select_ builds the object's query in a stable order so the start
// position walks every record once.
func (o *object) select_(where string, start, limit int) string {
	return strings.Join(strings.Fields(fmt.Sprintf("SELECT * FROM %s %s ORDERBY Id STARTPOSITION %d MAXRESULTS %d", o.entity, where, start, limit)), " ")
}

// count runs the object's count query and reads its total.
func (s *Source) count(object *object) (int, error) {
	body, err := s.get("/query", url.Values{"query": {"SELECT COUNT(*) FROM " + object.entity}})
	if err != nil {
		return 0, err
	}
	var answer struct {
		QueryResponse struct {
			TotalCount int `json:"totalCount"`
		} `json:"QueryResponse"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return 0, &Failure{Code: "source", Message: fmt.Sprintf("QuickBooks' count does not parse: %s.", err)}
	}
	return answer.QueryResponse.TotalCount, nil
}

// query runs one query and reads the entity's records out of its answer.
func (s *Source) query(soql, entity string) ([]record, error) {
	body, err := s.get("/query", url.Values{"query": {soql}})
	if err != nil {
		return nil, err
	}
	var answer struct {
		QueryResponse map[string]json.RawMessage `json:"QueryResponse"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("QuickBooks' answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := answer.QueryResponse[entity]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, &Failure{Code: "source", Message: fmt.Sprintf("QuickBooks' %s records do not parse: %s.", entity, err)}
		}
	}
	return items, nil
}

// get reads one company path with the query the caller adds, and the minor
// version every call names.
func (s *Source) get(path string, query url.Values) ([]byte, error) {
	query.Set("minorversion", MinorVersion)
	target := fmt.Sprintf("%s/v3/company/%s%s?%s", s.baseURL, url.PathEscape(s.realmID), path, query.Encode())
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

// signIn exchanges the app's refresh token for an access token once.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.clientID == "" || s.clientSecret == "" || s.refreshToken == "" || s.realmID == "" {
		return &Failure{Code: "not_connected", Message: "Connect QuickBooks with its client id, client secret, refresh token and realm id first."}
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
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("QuickBooks refused the app: %s. Connect QuickBooks again with a refresh token that is still valid.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("QuickBooks answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "QuickBooks' sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("QuickBooks did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("QuickBooks' answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status QuickBooks answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "QuickBooks refused the access token. Connect QuickBooks again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("QuickBooks refused the request: %s. The app needs the accounting scope for this company.", faultMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "QuickBooks asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("QuickBooks answered %d: %s", status, faultMessage(body))}
	}
	return nil
}

func faultMessage(body []byte) string {
	var failure struct {
		Fault struct {
			Error []struct {
				Message string `json:"Message"`
				Detail  string `json:"Detail"`
			} `json:"Error"`
		} `json:"Fault"`
	}
	if json.Unmarshal(body, &failure) == nil && len(failure.Fault.Error) > 0 {
		first := failure.Fault.Error[0]
		if first.Detail != "" {
			return first.Message + ", " + first.Detail
		}
		if first.Message != "" {
			return first.Message
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

func nestedWhen(parent, key string) func(record) string {
	return func(item record) string {
		if child, ok := item[parent].(map[string]any); ok {
			return formatTime(contract.Scalar(child[key]))
		}
		return ""
	}
}

// formatTime reads QuickBooks' `2023-11-15T03:00:00-08:00` as RFC 3339 in
// UTC.
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

// amount reads a number QuickBooks writes in the major unit as a decimal
// with two places.
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

// amountPaid is the invoice's total less its balance.
func amountPaid(item record) string {
	total, ok := item["TotalAmt"].(float64)
	if !ok {
		return ""
	}
	balance, _ := item["Balance"].(float64)
	return strconv.FormatFloat(total-balance, 'f', 2, 64)
}

// invoiceStatus folds an invoice's balance and due date onto a status:
// paid, overdue or open.
func invoiceStatus(item record) string {
	total, hasTotal := item["TotalAmt"].(float64)
	balance, hasBalance := item["Balance"].(float64)
	if !hasTotal && !hasBalance {
		return ""
	}
	if hasBalance && balance == 0 && total > 0 {
		return "paid"
	}
	if due, err := time.Parse("2006-01-02", contract.Scalar(item["DueDate"])); err == nil && due.Before(time.Now().UTC().Truncate(24*time.Hour)) {
		return "overdue"
	}
	return "open"
}

// linkedInvoice reads the first invoice a payment's lines apply to.
func linkedInvoice(item record) string {
	lines, _ := item["Line"].([]any)
	for _, entry := range lines {
		line, _ := entry.(map[string]any)
		linked, _ := line["LinkedTxn"].([]any)
		for _, candidate := range linked {
			txn, _ := candidate.(map[string]any)
			if contract.Scalar(txn["TxnType"]) == "Invoice" {
				return contract.Scalar(txn["TxnId"])
			}
		}
	}
	return ""
}

func emailDomain(item record) string {
	email := nested("PrimaryEmailAddr", "Address")(item)
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// websiteDomain reads a website as its registrable host, without scheme,
// `www.` or path.
func websiteDomain(item record) string {
	site := strings.TrimSpace(nested("WebAddr", "URI")(item))
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

// newestModified is the change mark: the latest LastUpdatedTime in a list
// as RFC 3339 in UTC with a numeric offset, the shape the query filter
// reads back, or the previous mark when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		meta, _ := item["MetaData"].(map[string]any)
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(meta["LastUpdatedTime"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(time.RFC3339Nano, previous); err == nil && before.After(newest) {
		return previous
	}
	return newest.UTC().Format("2006-01-02T15:04:05-07:00")
}
