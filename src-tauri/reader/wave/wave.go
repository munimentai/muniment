// Package wave reads a Wave business behind the reader contract: two
// objects, customers and invoices, each flattened to text fields a mapping
// can point at. It uses the public GraphQL API over net/http alone, with a
// full access token as a bearer token, and writes nothing back. Backfill
// walks numbered pages, and Delta is one query for the newest change after
// the cursor's mark.
package wave

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Wave's GraphQL host. A test points the reader
// elsewhere.
const DefaultBaseURL = "https://gql.waveapps.com"

// graphPath is the public GraphQL endpoint under the host.
const graphPath = "/graphql/public"

// PageLimit is the most records one page returns.
const PageLimit = 100

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

// Source is one Wave business reached through one full access token.
type Source struct {
	baseURL    string
	token      string
	businessID string
	client     *http.Client
}

// New opens a source on the credentials the panel packs: the full access
// token and the business id every query names. An empty base URL is the
// live API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	token := credentials["access_token"]
	if token == "" {
		token = credentials["token"]
	}
	return &Source{
		baseURL:    strings.TrimRight(baseURL, "/"),
		token:      token,
		businessID: credentials["business_id"],
		client:     &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one connection under the business: the field the query reads,
// the selection each node carries, the sort that walks it, the sort that
// reads the newest change first, and how each column reads.
type object struct {
	name      string
	label     string
	field     string
	selection string
	walk      string
	newest    string
	// since names the connection's filter for changes after a time, empty
	// when the connection has none.
	since  string
	fields []field
}

type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name: "customers", label: "Customers", field: "customers", walk: "CREATED_AT_ASC", newest: "MODIFIED_AT_DESC",
		selection: "id name firstName lastName email phone mobile website currency { code } address { addressLine1 city postalCode province { name code } country { code name } } createdAt modifiedAt",
		fields: []field{
			{"id", "id", text("id")},
			{"name", "string", text("name")},
			{"first_name", "string", text("firstName")},
			{"last_name", "string", text("lastName")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain},
			{"phone", "phone", firstOf("phone", "mobile")},
			{"website", "string", text("website")},
			{"currency", "string", nested("currency", "code")},
			{"street", "string", nested("address", "addressLine1")},
			{"city", "string", nested("address", "city")},
			{"state", "string", province},
			{"postal_code", "string", nested("address", "postalCode")},
			{"country", "string", country},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("modifiedAt")},
		},
	},
	{
		name: "invoices", label: "Invoices", field: "invoices", walk: "CREATED_AT_ASC", newest: "MODIFIED_AT_DESC", since: "modifiedAtAfter",
		selection: "id invoiceNumber title status invoiceDate dueDate currency { code } total { value } amountDue { value } amountPaid { value } customer { id name email } memo viewUrl createdAt modifiedAt",
		fields: []field{
			{"id", "id", text("id")},
			{"number", "string", text("invoiceNumber")},
			{"title", "string", text("title")},
			{"customer_id", "id", nested("customer", "id")},
			{"customer_name", "string", nested("customer", "name")},
			{"customer_email", "email", nested("customer", "email")},
			{"status", "string", lowered("status")},
			{"total", "number", money("total")},
			{"amount_due", "number", money("amountDue")},
			{"amount_paid", "number", money("amountPaid")},
			{"currency", "string", nested("currency", "code")},
			{"date", "date", text("invoiceDate")},
			{"due_date", "date", text("dueDate")},
			{"memo", "string", text("memo")},
			{"view_url", "string", text("viewUrl")},
			{"created", "date-time", when("createdAt")},
			{"modified", "date-time", when("modifiedAt")},
		},
	},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Wave has no %s to read. The objects are customers and invoices.", name)}
}

// Objects proves the token and the business with one small query and
// lists the two.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if s.token == "" || s.businessID == "" {
		return nil, &Failure{Code: "not_connected", Message: "Connect Wave with its full access token and business id first."}
	}
	if _, _, err := s.list(&objects[0], 1, 1, objects[0].walk, ""); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their samples.
// The connection names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	items, info, err := s.list(object, 1, sampleRows, object.walk, "")
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
	return &Description{Source: "wave", Object: name, Label: object.label, Fields: contract.Sample(columns, rows), Rows: info.total, Hash: newestModified(items, ""), Counted: true}, nil
}

// Page reads one numbered page, the cursor's token or the first, in
// creation order so every record lands once. Total is the connection's
// own count.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
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
	items, info, err := s.list(object, page, limit, object.walk, "")
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	hash = newestModified(items, hash)
	answer := &Page{Rows: rows, Offset: offset, Total: info.total, Hash: hash, Counted: true}
	if info.page < info.pages {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(info.page + 1)}
	}
	return answer, nil
}

// Delta asks for the one record modified last, after the mark where the
// connection filters by time, and compares it with the mark.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	since := ""
	if object.since != "" {
		since = mark
	}
	items, _, err := s.list(object, 1, 1, object.newest, since)
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

// pageInfo is where one page stands in its connection.
type pageInfo struct {
	page  int
	pages int
	total int
}

// list runs one connection query and answers its nodes and page info.
func (s *Source) list(object *object, page, size int, sort, since string) ([]record, pageInfo, error) {
	arguments := fmt.Sprintf("page: $page, pageSize: $pageSize, sort: [%s]", sort)
	variables := map[string]any{"business": s.businessID, "page": page, "pageSize": size}
	declared := "$business: ID!, $page: Int!, $pageSize: Int!"
	if since != "" {
		arguments += fmt.Sprintf(", %s: $since", object.since)
		declared += ", $since: DateTime!"
		variables["since"] = since
	}
	query := fmt.Sprintf("query Read(%s) { business(id: $business) { id %s(%s) { pageInfo { currentPage totalPages totalCount } edges { node { %s } } } } }", declared, object.field, arguments, object.selection)
	body, err := s.post(map[string]any{"query": query, "variables": variables})
	if err != nil {
		return nil, pageInfo{}, err
	}
	var answer struct {
		Data struct {
			Business *map[string]json.RawMessage `json:"business"`
		} `json:"data"`
		Errors []struct {
			Message string `json:"message"`
		} `json:"errors"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, pageInfo{}, &Failure{Code: "source", Message: fmt.Sprintf("Wave's answer does not parse: %s.", err)}
	}
	if len(answer.Errors) > 0 {
		return nil, pageInfo{}, &Failure{Code: "source", Message: fmt.Sprintf("Wave answered an error: %s", answer.Errors[0].Message)}
	}
	if answer.Data.Business == nil {
		return nil, pageInfo{}, &Failure{Code: "not_connected", Message: "Wave found no business with that id for this token. Connect Wave again with the id of a business the token can read."}
	}
	var connection struct {
		PageInfo struct {
			CurrentPage int `json:"currentPage"`
			TotalPages  int `json:"totalPages"`
			TotalCount  int `json:"totalCount"`
		} `json:"pageInfo"`
		Edges []struct {
			Node record `json:"node"`
		} `json:"edges"`
	}
	if raw, ok := (*answer.Data.Business)[object.field]; ok {
		if err := json.Unmarshal(raw, &connection); err != nil {
			return nil, pageInfo{}, &Failure{Code: "source", Message: fmt.Sprintf("Wave's %s do not parse: %s.", object.name, err)}
		}
	}
	items := make([]record, 0, len(connection.Edges))
	for _, edge := range connection.Edges {
		if edge.Node != nil {
			items = append(items, edge.Node)
		}
	}
	info := pageInfo{page: connection.PageInfo.CurrentPage, pages: connection.PageInfo.TotalPages, total: connection.PageInfo.TotalCount}
	if info.page == 0 {
		info.page = page
	}
	return items, info, nil
}

// post sends one GraphQL request and reads its body after the refusals.
func (s *Source) post(payload map[string]any) ([]byte, error) {
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, &Failure{Code: "source", Message: err.Error()}
	}
	request, err := http.NewRequest(http.MethodPost, s.baseURL+graphPath, bytes.NewReader(encoded))
	if err != nil {
		return nil, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, &Failure{Code: "unreachable", Message: fmt.Sprintf("Wave did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, &Failure{Code: "unreachable", Message: fmt.Sprintf("Wave's answer did not read: %s.", err)}
	}
	if err := refusal(response.StatusCode, body); err != nil {
		return nil, err
	}
	return body, nil
}

// refusal turns a status Wave answers into the one failure the runtime
// reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized || status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: "Wave refused the access token. Connect Wave again with a full access token that works."}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Wave asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Wave answered %d: %s", status, waveMessage(body))}
	}
	return nil
}

func waveMessage(body []byte) string {
	var failure struct {
		Errors []struct {
			Message string `json:"message"`
		} `json:"errors"`
		Detail string `json:"detail"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if len(failure.Errors) > 0 && failure.Errors[0].Message != "" {
			return failure.Errors[0].Message
		}
		if failure.Detail != "" {
			return failure.Detail
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

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Wave's ISO 8601 time as RFC 3339 in UTC.
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

// money reads a Wave money object as its decimal value in the major unit,
// whether Wave writes the value as text or as a number.
func money(key string) func(record) string {
	return func(item record) string {
		amount, ok := item[key].(map[string]any)
		if !ok {
			return ""
		}
		switch value := amount["value"].(type) {
		case string:
			return value
		case float64:
			return strconv.FormatFloat(value, 'f', 2, 64)
		default:
			return ""
		}
	}
}

// province reads the state or province name under the address.
func province(item record) string {
	address, ok := item["address"].(map[string]any)
	if !ok {
		return ""
	}
	if child, ok := address["province"].(map[string]any); ok {
		if name := contract.Scalar(child["name"]); name != "" {
			return name
		}
		return contract.Scalar(child["code"])
	}
	return ""
}

// country reads the country code under the address.
func country(item record) string {
	address, ok := item["address"].(map[string]any)
	if !ok {
		return ""
	}
	if child, ok := address["country"].(map[string]any); ok {
		return contract.Scalar(child["code"])
	}
	return ""
}

func emailDomain(item record) string {
	email := contract.Scalar(item["email"])
	if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
		return strings.ToLower(email[at+1:])
	}
	return ""
}

// newestModified is the change mark: the latest modifiedAt in a list as
// RFC 3339 in UTC, the shape the modifiedAtAfter filter reads back, or the
// previous mark when nothing newer appears.
func newestModified(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["modifiedAt"])); err == nil && parsed.After(newest) {
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
