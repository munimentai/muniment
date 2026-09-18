// Package shopify reads a Shopify store behind the reader contract: three
// objects, customers, orders and products, each flattened to text fields a
// mapping can point at. It uses the Admin REST API over net/http alone,
// with a custom app's Admin API access token in the store's header, and it
// writes nothing back. Backfill walks cursor pagination through the Link
// header, and Delta is one query for the records updated after the
// cursor's mark.
package shopify

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

// APIVersion is the Admin API version every path names.
const APIVersion = "2024-07"

// PageLimit is the most records one list call returns.
const PageLimit = 250

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 250

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

// Source is one Shopify store reached through one custom app.
type Source struct {
	baseURL   string
	shop      string
	token     string
	client    *http.Client
	lastRetry string
	lastLink  string
}

// New opens a source on the credentials the panel packs: the store's
// myshopify domain and the custom app's Admin API access token. A base URL
// points a test at a fake.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	shop := shopName(credentials["shop"])
	if baseURL == "" && shop != "" {
		baseURL = "https://" + shop + ".myshopify.com"
	}
	return &Source{
		baseURL: strings.TrimRight(baseURL, "/"),
		shop:    shop,
		token:   credentials["access_token"],
		client:  &http.Client{Timeout: 30 * time.Second},
	}
}

// shopName reads a pasted store address as its bare myshopify name.
func shopName(value string) string {
	shop := strings.TrimSpace(strings.ToLower(value))
	shop = strings.TrimPrefix(strings.TrimPrefix(shop, "https://"), "http://")
	if slash := strings.Index(shop, "/"); slash >= 0 {
		shop = shop[:slash]
	}
	return strings.TrimSuffix(shop, ".myshopify.com")
}

type record = map[string]any

// object is one Admin API list: the resource whose path and answer key
// share its name, the query every first page carries, and its fields.
type object struct {
	name     string
	label    string
	resource string
	query    url.Values
	fields   []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "customers", label: "Customers", resource: "customers",
		fields: []field{
			{"id", "id", text("id")},
			{"first_name", "string", text("first_name")},
			{"last_name", "string", text("last_name")},
			{"name", "string", fullName("first_name", "last_name")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain("email")},
			{"phone", "phone", text("phone")},
			{"state", "string", text("state")},
			{"verified_email", "boolean", boolean("verified_email")},
			{"tax_exempt", "boolean", boolean("tax_exempt")},
			{"orders_count", "number", text("orders_count")},
			{"total_spent", "number", text("total_spent")},
			{"currency", "string", text("currency")},
			{"company", "string", nested("default_address", "company")},
			{"city", "string", nested("default_address", "city")},
			{"province", "string", nested("default_address", "province")},
			{"country", "string", nested("default_address", "country_code")},
			{"postal_code", "string", nested("default_address", "zip")},
			{"marketing", "string", nested("email_marketing_consent", "state")},
			{"note", "string", text("note")},
			{"tags", "string", tags("tags")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "orders", label: "Orders", resource: "orders",
		// Shopify lists open orders alone by default, so every first page
		// asks for any status.
		query: url.Values{"status": {"any"}},
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"order_number", "number", text("order_number")},
			{"customer_id", "id", nested("customer", "id")},
			{"customer_name", "string", customerName},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain("email")},
			{"status", "string", orderStatus},
			{"financial_status", "string", text("financial_status")},
			{"fulfillment_status", "string", text("fulfillment_status")},
			{"total", "number", text("total_price")},
			{"subtotal", "number", text("subtotal_price")},
			{"tax", "number", text("total_tax")},
			{"discounts", "number", text("total_discounts")},
			{"currency", "string", text("currency")},
			{"line_items", "number", count("line_items")},
			{"source", "string", text("source_name")},
			{"tags", "string", tags("tags")},
			{"note", "string", text("note")},
			{"cancel_reason", "string", text("cancel_reason")},
			{"test", "boolean", boolean("test")},
			{"created", "date-time", when("created_at")},
			{"processed_at", "date-time", when("processed_at")},
			{"closed_at", "date-time", when("closed_at")},
			{"cancelled_at", "date-time", when("cancelled_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name: "products", label: "Products", resource: "products",
		fields: []field{
			{"id", "id", text("id")},
			{"title", "string", text("title")},
			{"handle", "string", text("handle")},
			{"vendor", "string", text("vendor")},
			{"product_type", "string", text("product_type")},
			{"status", "string", text("status")},
			{"tags", "string", tags("tags")},
			{"price", "number", firstVariant("price")},
			{"sku", "string", firstVariant("sku")},
			{"variants", "number", count("variants")},
			{"inventory", "number", inventory},
			{"published_at", "date-time", when("published_at")},
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
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Shopify has no %s to read. The objects are customers, orders and products.", name)}
}

// Objects proves the token by reading the store itself and lists the three.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.baseURL == "" || s.token == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Shopify with its myshopify domain and Admin API access token first."}
	}
	body, status, err := s.get(s.path("shop"), nil)
	if err != nil {
		return nil, err
	}
	if err := s.refusal("shop", status, body); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// The store counts its records, so Rows is the count and Counted is true
// when the count call answers.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	query := cloneValues(object.query)
	query.Set("limit", strconv.Itoa(sampleRows))
	items, _, err := s.list(object, query)
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
	total, counted := s.count(object)
	if !counted {
		total = len(rows)
	}
	return &Description{
		Source:  "shopify",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(columns, rows),
		Rows:    total,
		Bytes:   0,
		Hash:    newestUpdated(items, ""),
		Counted: counted,
	}, nil
}

// Page reads one page: the first through the object's query, the rest
// through the page_info token the Link header named. Shopify allows no
// filter beside a page_info, so a later page carries the limit alone.
// Total is the rows seen so far.
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
	query := url.Values{}
	if token != "" {
		query.Set("page_info", token)
	} else {
		query = cloneValues(object.query)
	}
	query.Set("limit", strconv.Itoa(limit))
	items, next, err := s.list(object, query)
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

// Delta asks for the records updated at or after the mark. The filter is
// inclusive, so the record at the mark itself never counts as a change.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	query := cloneValues(object.query)
	query.Set("limit", strconv.Itoa(PageLimit))
	if mark != "" {
		query.Set("updated_at_min", mark)
	}
	items, _, err := s.list(object, query)
	if err != nil {
		return nil, err
	}
	newest := newestUpdated(items, mark)
	if newest == mark && mark != "" {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// count reads the store's own count of an object. A store whose app cannot
// count reads as uncounted, because the rows still land.
func (s *Source) count(object *object) (int, bool) {
	body, status, err := s.get(s.path(object.resource+"/count"), cloneValues(object.query))
	if err != nil || status != http.StatusOK {
		return 0, false
	}
	var counted struct {
		Count *int `json:"count"`
	}
	if json.Unmarshal(body, &counted) != nil || counted.Count == nil {
		return 0, false
	}
	return *counted.Count, true
}

// list fetches one page and answers its records and the page_info of the
// next page, read from the Link header.
func (s *Source) list(object *object, query url.Values) ([]record, string, error) {
	body, status, err := s.get(s.path(object.resource), query)
	if err != nil {
		return nil, "", err
	}
	if err := s.refusal(object.name, status, body); err != nil {
		return nil, "", err
	}
	var listed map[string]json.RawMessage
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Shopify's answer does not parse: %s.", err)}
	}
	var items []record
	if raw, ok := listed[object.resource]; ok {
		if err := json.Unmarshal(raw, &items); err != nil {
			return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Shopify's %s do not parse: %s.", object.resource, err)}
		}
	}
	return items, nextPageInfo(s.lastLink), nil
}

var linkPart = regexp.MustCompile(`<([^>]+)>\s*;\s*rel="([^"]+)"`)

// nextPageInfo reads the page_info of the link marked next out of a Link
// header, or nothing when the page is the last.
func nextPageInfo(header string) string {
	for _, match := range linkPart.FindAllStringSubmatch(header, -1) {
		if match[2] != "next" {
			continue
		}
		parsed, err := url.Parse(match[1])
		if err != nil {
			return ""
		}
		return parsed.Query().Get("page_info")
	}
	return ""
}

func (s *Source) path(resource string) string {
	return "/admin/api/" + APIVersion + "/" + resource + ".json"
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
	request.Header.Set("X-Shopify-Access-Token", s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Shopify did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Shopify's answer did not read: %s.", err)}
	}
	s.lastRetry = response.Header.Get("Retry-After")
	s.lastLink = response.Header.Get("Link")
	return body, response.StatusCode, nil
}

// refusal turns a status Shopify answers into the one failure the runtime
// reads. A 403 names a scope the custom app lacks.
func (s *Source) refusal(object string, status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Shopify refused the access token. Connect Shopify again with a custom app token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Shopify refused the request for %s: %s. The custom app needs the read_%s scope.", object, shopifyMessage(body), object)}
	case status == http.StatusTooManyRequests:
		if seconds, err := strconv.ParseFloat(strings.TrimSpace(s.lastRetry), 64); err == nil && seconds > 0 {
			return &Failure{Code: "rate_limited", Message: fmt.Sprintf("Shopify asked the reader to wait %d seconds. Run again then.", int(seconds+0.5))}
		}
		return &Failure{Code: "rate_limited", Message: "Shopify asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Shopify answered %d: %s", status, shopifyMessage(body))}
	}
	return nil
}

func shopifyMessage(body []byte) string {
	var failure struct {
		Errors any `json:"errors"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if text := contract.Scalar(failure.Errors); text != "" {
			return contract.Clip(text, 200)
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

func count(key string) func(record) string {
	return func(item record) string {
		list, ok := item[key].([]any)
		if !ok {
			return ""
		}
		return strconv.Itoa(len(list))
	}
}

// tags reads Shopify's comma separated tag text as its tags sorted and
// joined by commas, so a tag list reads the same whatever order the store
// used.
func tags(key string) func(record) string {
	return func(item record) string {
		parts := []string{}
		for _, part := range strings.Split(contract.Scalar(item[key]), ",") {
			if part = strings.TrimSpace(part); part != "" {
				parts = append(parts, part)
			}
		}
		sort.Strings(parts)
		return strings.Join(parts, ",")
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Shopify's offset stamps as RFC 3339 in UTC.
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

func customerName(item record) string {
	customer, ok := item["customer"].(map[string]any)
	if !ok {
		return ""
	}
	return fullName("first_name", "last_name")(customer)
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

// orderStatus reads the order's own state the way the status filter names
// it: cancelled, closed or open.
func orderStatus(item record) string {
	if contract.Scalar(item["cancelled_at"]) != "" {
		return "cancelled"
	}
	if contract.Scalar(item["closed_at"]) != "" {
		return "closed"
	}
	return "open"
}

func variants(item record) []map[string]any {
	list, ok := item["variants"].([]any)
	if !ok {
		return nil
	}
	out := make([]map[string]any, 0, len(list))
	for _, entry := range list {
		if variant, ok := entry.(map[string]any); ok {
			out = append(out, variant)
		}
	}
	return out
}

func firstVariant(key string) func(record) string {
	return func(item record) string {
		list := variants(item)
		if len(list) == 0 {
			return ""
		}
		return contract.Scalar(list[0][key])
	}
}

// inventory sums the variants' inventory quantities.
func inventory(item record) string {
	list := variants(item)
	if len(list) == 0 {
		return ""
	}
	var total float64
	for _, variant := range list {
		if quantity, ok := variant["inventory_quantity"].(float64); ok {
			total += quantity
		}
	}
	return contract.Scalar(total)
}

// newestUpdated is the change mark: the latest updated_at in a list as RFC
// 3339 in UTC, so updated_at_min reads it back, or the previous mark when
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

func cloneValues(values url.Values) url.Values {
	out := url.Values{}
	for key, list := range values {
		out[key] = append([]string(nil), list...)
	}
	return out
}
