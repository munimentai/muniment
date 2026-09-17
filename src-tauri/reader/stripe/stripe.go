// Package stripe reads a Stripe account behind the reader contract: three
// objects, customers, subscriptions and invoices, each flattened to text
// fields a mapping can point at. It uses the REST API over net/http alone,
// with the secret key as a bearer token, and it writes nothing back.
package stripe

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

// DefaultBaseURL is Stripe's API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://api.stripe.com"

// PageLimit is the most objects one Stripe list call returns.
const PageLimit = 100

// sampleRows is how many objects Describe reads for its samples.
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

// Source is one Stripe account.
type Source struct {
	secret  string
	baseURL string
	client  *http.Client
}

// New opens a source on a secret key. An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		secret:  secret,
		baseURL: strings.TrimRight(baseURL, "/"),
		client:  &http.Client{Timeout: 30 * time.Second},
	}
}

// object is one Stripe list endpoint and the fields its rows carry, in the
// order Describe lists them.
type object struct {
	name   string
	label  string
	path   string
	query  url.Values
	fields []field
}

// field names one text column and how it reads out of a Stripe object.
type field struct {
	name  string
	guess string
	read  func(map[string]any) string
}

var objects = []object{
	{
		name:  "customers",
		label: "Customers",
		path:  "/v1/customers",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", text("phone")},
			{"description", "string", text("description")},
			{"created", "date-time", unixTime("created")},
			{"currency", "string", text("currency")},
			{"balance", "number", cents("balance", "currency")},
			{"delinquent", "boolean", boolean("delinquent")},
			{"address_line1", "string", nested("address", "line1")},
			{"address_city", "string", nested("address", "city")},
			{"address_state", "string", nested("address", "state")},
			{"address_postal_code", "string", nested("address", "postal_code")},
			{"address_country", "string", nested("address", "country")},
			{"livemode", "boolean", boolean("livemode")},
		},
	},
	{
		name:  "subscriptions",
		label: "Subscriptions",
		path:  "/v1/subscriptions",
		// The product rides along expanded, so the plan reads as the product's
		// name and never as its id.
		query: url.Values{"status": {"all"}, "expand[]": {"data.items.data.price.product"}},
		fields: []field{
			{"id", "id", text("id")},
			{"customer", "id", customerID},
			{"status", "string", text("status")},
			{"state", "string", subscriptionState},
			{"plan", "string", subscriptionPlan},
			{"billing_period", "string", subscriptionInterval},
			{"amount", "number", subscriptionAmount},
			{"currency", "string", text("currency")},
			{"created", "date-time", unixTime("created")},
			{"started_at", "date-time", unixTime("start_date")},
			{"current_period_start", "date-time", unixTime("current_period_start")},
			{"current_period_end", "date-time", unixTime("current_period_end")},
			{"cancelled_at", "date-time", unixTime("canceled_at")},
			{"cancel_at_period_end", "boolean", boolean("cancel_at_period_end")},
			{"livemode", "boolean", boolean("livemode")},
		},
	},
	{
		name:  "invoices",
		label: "Invoices",
		path:  "/v1/invoices",
		fields: []field{
			{"id", "id", text("id")},
			{"number", "string", text("number")},
			{"customer", "id", customerID},
			{"customer_email", "email", text("customer_email")},
			{"customer_name", "string", text("customer_name")},
			{"status", "string", text("status")},
			{"amount_due", "number", cents("amount_due", "currency")},
			{"amount_paid", "number", cents("amount_paid", "currency")},
			{"total", "number", cents("total", "currency")},
			{"currency", "string", text("currency")},
			{"created", "date-time", unixTime("created")},
			{"due_date", "date-time", unixTime("due_date")},
			{"paid_at", "date-time", nestedUnixTime("status_transitions", "paid_at")},
			{"hosted_invoice_url", "string", text("hosted_invoice_url")},
			{"livemode", "boolean", boolean("livemode")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Stripe has no %s to read. The objects are customers, subscriptions and invoices.", name)}
}

// Objects lists the three objects the reader knows.
func (s *Source) Objects() ([]ObjectInfo, error) {
	// One cheap call proves the key before the panel offers the objects.
	if _, _, err := s.list("/v1/customers", url.Values{"limit": {"1"}}); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// Stripe counts nothing, so Rows is the rows read and Counted is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	query := cloneValues(object.query)
	query.Set("limit", strconv.Itoa(sampleRows))
	items, _, err := s.list(object.path, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	fields := make([]FieldDescription, 0, len(object.fields))
	for _, f := range object.fields {
		var samples []string
		seen := map[string]bool{}
		filled := 0
		for _, row := range rows {
			value := row[f.name]
			if value == "" {
				continue
			}
			filled++
			if len(samples) < 3 && !seen[value] {
				seen[value] = true
				samples = append(samples, clip(value, 80))
			}
		}
		if samples == nil {
			samples = []string{}
		}
		fields = append(fields, FieldDescription{Name: f.name, Guess: f.guess, Samples: samples, Filled: filled})
	}
	return &Description{
		Source:  "stripe",
		Object:  name,
		Label:   object.label,
		Fields:  fields,
		Rows:    len(rows),
		Bytes:   0,
		Hash:    newestCreated(items, ""),
		Counted: false,
	}, nil
}

// Page reads one Stripe page after the cursor's token. Total is the rows
// seen so far, because Stripe counts nothing.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	query := cloneValues(object.query)
	query.Set("limit", strconv.Itoa(limit))
	offset := 0
	hash := ""
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		if cursor.Token != "" {
			query.Set("starting_after", cursor.Token)
		}
	}
	items, hasMore, err := s.list(object.path, query)
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestCreated(items, hash)
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if hasMore && len(items) > 0 {
		last, _ := items[len(items)-1]["id"].(string)
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: last}
	}
	return page, nil
}

// Delta reads the first page and compares its newest creation time with the
// cursor's mark. Stripe lists carry no change filter, so a changed field on
// an old object reads as unchanged here, and the run lands it anyway.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	query := cloneValues(object.query)
	query.Set("limit", strconv.Itoa(PageLimit))
	items, _, err := s.list(object.path, query)
	if err != nil {
		return nil, err
	}
	newest := newestCreated(items, "")
	if cursor != nil && newest == cursor.Hash {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// list fetches one page of a list endpoint and answers its data and has_more.
func (s *Source) list(path string, query url.Values) ([]map[string]any, bool, error) {
	request, err := http.NewRequest(http.MethodGet, s.baseURL+path+"?"+query.Encode(), nil)
	if err != nil {
		return nil, false, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.secret)
	request.Header.Set("Stripe-Version", "2024-06-20")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, false, &Failure{Code: "unreachable", Message: fmt.Sprintf("Stripe did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, false, &Failure{Code: "unreachable", Message: fmt.Sprintf("Stripe's answer did not read: %s.", err)}
	}
	if response.StatusCode == http.StatusUnauthorized {
		return nil, false, &Failure{Code: "not_connected", Message: "Stripe refused the secret key. Connect Stripe again with a key that works."}
	}
	if response.StatusCode == http.StatusTooManyRequests {
		return nil, false, &Failure{Code: "rate_limited", Message: "Stripe asked the reader to wait. Run again in a minute."}
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		return nil, false, &Failure{Code: "source", Message: fmt.Sprintf("Stripe answered %d: %s", response.StatusCode, stripeMessage(body))}
	}
	var listed struct {
		Data    []map[string]any `json:"data"`
		HasMore bool             `json:"has_more"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, false, &Failure{Code: "source", Message: fmt.Sprintf("Stripe's answer does not parse: %s.", err)}
	}
	return listed.Data, listed.HasMore, nil
}

func stripeMessage(body []byte) string {
	var failure struct {
		Error struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error.Message != "" {
		return failure.Error.Message
	}
	return clip(strings.TrimSpace(string(body)), 200)
}

func (o *object) row(item map[string]any) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	// Metadata keys land as their own columns, so a mapping can point at them.
	if metadata, ok := item["metadata"].(map[string]any); ok {
		keys := make([]string, 0, len(metadata))
		for key := range metadata {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			row["metadata_"+key] = scalar(metadata[key])
		}
	}
	return row
}

func text(key string) func(map[string]any) string {
	return func(item map[string]any) string { return scalar(item[key]) }
}

func nested(parent, key string) func(map[string]any) string {
	return func(item map[string]any) string {
		if child, ok := item[parent].(map[string]any); ok {
			return scalar(child[key])
		}
		return ""
	}
}

func boolean(key string) func(map[string]any) string {
	return func(item map[string]any) string {
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

// unixTime reads a Unix second count as RFC 3339 in UTC.
func unixTime(key string) func(map[string]any) string {
	return func(item map[string]any) string { return formatUnix(item[key]) }
}

func nestedUnixTime(parent, key string) func(map[string]any) string {
	return func(item map[string]any) string {
		if child, ok := item[parent].(map[string]any); ok {
			return formatUnix(child[key])
		}
		return ""
	}
}

func formatUnix(value any) string {
	seconds, ok := value.(float64)
	if !ok || seconds <= 0 {
		return ""
	}
	return time.Unix(int64(seconds), 0).UTC().Format(time.RFC3339)
}

// cents reads a minor-unit amount as a decimal in the major unit. A zero
// decimal currency such as JPY keeps its whole number.
func cents(key, currencyKey string) func(map[string]any) string {
	return func(item map[string]any) string {
		amount, ok := item[key].(float64)
		if !ok {
			return ""
		}
		return formatAmount(amount, scalar(item[currencyKey]))
	}
}

var zeroDecimalCurrencies = map[string]bool{
	"bif": true, "clp": true, "djf": true, "gnf": true, "jpy": true, "kmf": true, "krw": true,
	"mga": true, "pyg": true, "rwf": true, "ugx": true, "vnd": true, "vuv": true, "xaf": true,
	"xof": true, "xpf": true,
}

func formatAmount(minor float64, currency string) string {
	if zeroDecimalCurrencies[strings.ToLower(currency)] {
		return strconv.FormatFloat(minor, 'f', 0, 64)
	}
	return strconv.FormatFloat(minor/100, 'f', 2, 64)
}

func emailDomain(item map[string]any) string {
	email := scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// customerID reads a customer field that is an id or an expanded object.
func customerID(item map[string]any) string {
	switch value := item["customer"].(type) {
	case string:
		return value
	case map[string]any:
		return scalar(value["id"])
	default:
		return ""
	}
}

// subscriptionState folds Stripe's statuses onto the subscription kind's
// states: trial, active, paused or cancelled.
func subscriptionState(item map[string]any) string {
	switch scalar(item["status"]) {
	case "trialing":
		return "trial"
	case "active", "past_due", "unpaid", "incomplete":
		return "active"
	case "paused":
		return "paused"
	case "canceled", "incomplete_expired":
		return "cancelled"
	default:
		return ""
	}
}

func firstItem(item map[string]any) map[string]any {
	items, ok := item["items"].(map[string]any)
	if !ok {
		return nil
	}
	data, ok := items["data"].([]any)
	if !ok || len(data) == 0 {
		return nil
	}
	first, _ := data[0].(map[string]any)
	return first
}

func firstPrice(item map[string]any) map[string]any {
	first := firstItem(item)
	if first == nil {
		return nil
	}
	price, _ := first["price"].(map[string]any)
	return price
}

func subscriptionPlan(item map[string]any) string {
	price := firstPrice(item)
	if price == nil {
		return ""
	}
	if nickname := scalar(price["nickname"]); nickname != "" {
		return nickname
	}
	switch product := price["product"].(type) {
	case string:
		return product
	case map[string]any:
		if name := scalar(product["name"]); name != "" {
			return name
		}
		return scalar(product["id"])
	}
	return scalar(price["id"])
}

func subscriptionInterval(item map[string]any) string {
	price := firstPrice(item)
	if price == nil {
		return ""
	}
	recurring, ok := price["recurring"].(map[string]any)
	if !ok {
		return ""
	}
	interval := scalar(recurring["interval"])
	count, _ := recurring["interval_count"].(float64)
	if count > 1 {
		return fmt.Sprintf("%d %ss", int(count), interval)
	}
	return interval
}

// subscriptionAmount sums every item's unit amount times its quantity.
func subscriptionAmount(item map[string]any) string {
	items, ok := item["items"].(map[string]any)
	if !ok {
		return ""
	}
	data, ok := items["data"].([]any)
	if !ok || len(data) == 0 {
		return ""
	}
	var minor float64
	currency := scalar(item["currency"])
	for _, entry := range data {
		line, _ := entry.(map[string]any)
		price, _ := line["price"].(map[string]any)
		unit, _ := price["unit_amount"].(float64)
		quantity, ok := line["quantity"].(float64)
		if !ok {
			quantity = 1
		}
		minor += unit * quantity
	}
	return formatAmount(minor, currency)
}

func scalar(value any) string {
	switch typed := value.(type) {
	case nil:
		return ""
	case string:
		return typed
	case bool:
		if typed {
			return "yes"
		}
		return "no"
	case float64:
		if typed == float64(int64(typed)) {
			return strconv.FormatInt(int64(typed), 10)
		}
		return strconv.FormatFloat(typed, 'f', -1, 64)
	default:
		encoded, err := json.Marshal(typed)
		if err != nil {
			return ""
		}
		return string(encoded)
	}
}

// newestCreated is the change mark: the largest created time in a list, as
// text, or the previous mark when the list is empty.
func newestCreated(items []map[string]any, previous string) string {
	newest := 0.0
	for _, item := range items {
		if created, ok := item["created"].(float64); ok && created > newest {
			newest = created
		}
	}
	if newest == 0 {
		return previous
	}
	if before, err := strconv.ParseFloat(previous, 64); err == nil && before > newest {
		return previous
	}
	return strconv.FormatInt(int64(newest), 10)
}

func cloneValues(values url.Values) url.Values {
	out := url.Values{}
	for key, list := range values {
		out[key] = append([]string(nil), list...)
	}
	return out
}

func clip(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max])
}
