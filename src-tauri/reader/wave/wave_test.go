package wave

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strings"
	"testing"
)

var (
	fieldClause = regexp.MustCompile(`id (\w+)\(page:`)
	sortClause  = regexp.MustCompile(`sort: \[(\w+)\]`)
)

// fakeWave reads the operation out of each GraphQL request: the connection
// it names, its sort and its since filter, and the variables. It answers
// numbered pages from fixtures, null for a business the token cannot read,
// and refuses a wrong token.
func fakeWave(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	customers := []map[string]any{
		{"id": "c1", "name": "Northwind Traders", "firstName": "Ann", "lastName": "Lee", "email": "Ann@Northwind.example", "phone": "", "mobile": "+1 555 010 0000", "website": "https://northwind.example", "currency": map[string]any{"code": "USD"},
			"address":   map[string]any{"addressLine1": "1 Main St", "city": "Seattle", "postalCode": "98101", "province": map[string]any{"name": "Washington", "code": "US-WA"}, "country": map[string]any{"code": "US", "name": "United States"}},
			"createdAt": "2023-01-01T08:00:00.000Z", "modifiedAt": "2023-11-15T11:00:00.000Z"},
		{"id": "c2", "name": "Contoso", "email": "bob@contoso.example", "phone": "+1 555 010 0001", "currency": map[string]any{"code": "EUR"}, "address": map[string]any{"province": map[string]any{"code": "DE-BE"}}, "createdAt": "2023-02-01T08:00:00.000Z", "modifiedAt": "2023-11-14T11:00:00.000Z"},
		{"id": "c3", "name": "Fabrikam", "createdAt": "2023-03-01T08:00:00.000Z", "modifiedAt": "2023-11-13T11:00:00.000Z"},
	}
	invoices := []map[string]any{
		{"id": "i1", "invoiceNumber": "0001", "title": "Invoice", "status": "PARTIAL", "invoiceDate": "2023-11-01", "dueDate": "2023-12-01", "currency": map[string]any{"code": "USD"}, "total": map[string]any{"value": "1200.00"}, "amountDue": map[string]any{"value": "200.00"}, "amountPaid": map[string]any{"value": "1000.00"},
			"customer": map[string]any{"id": "c1", "name": "Northwind Traders", "email": "ann@northwind.example"}, "memo": "Thank you", "viewUrl": "https://wave.example/i1", "createdAt": "2023-11-01T08:00:00.000Z", "modifiedAt": "2023-11-16T08:00:00.000Z"},
		{"id": "i2", "invoiceNumber": "0002", "status": "PAID", "invoiceDate": "2023-11-02", "dueDate": "2023-11-20", "currency": map[string]any{"code": "EUR"}, "total": map[string]any{"value": 50.0}, "amountDue": map[string]any{"value": 0.0}, "amountPaid": map[string]any{"value": 50.0},
			"customer": map[string]any{"id": "c2", "name": "Contoso"}, "createdAt": "2023-11-02T08:00:00.000Z", "modifiedAt": "2023-11-10T08:00:00.000Z"},
	}
	data := map[string][]map[string]any{"customers": customers, "invoices": invoices}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var request struct {
			Query     string         `json:"query"`
			Variables map[string]any `json:"variables"`
		}
		_ = json.Unmarshal(body, &request)
		calls = append(calls, r.Method+" "+r.URL.Path+" "+request.Query+" "+string(mustJSON(request.Variables)))
		if r.Method != http.MethodPost || r.URL.Path != graphPath {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"detail":"Not found."}`))
			return
		}
		if r.Header.Get("Authorization") != "Bearer good" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"detail":"Invalid token."}`))
			return
		}
		if request.Variables["business"] != "biz1" {
			_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"business": nil}})
			return
		}
		match := fieldClause.FindStringSubmatch(request.Query)
		if match == nil {
			_ = json.NewEncoder(w).Encode(map[string]any{"errors": []any{map[string]any{"message": "Cannot query field"}}})
			return
		}
		rows := data[match[1]]
		if since, ok := request.Variables["since"].(string); ok && strings.Contains(request.Query, "modifiedAtAfter: $since") {
			kept := []map[string]any{}
			for _, row := range rows {
				if formatTime(row["modifiedAt"].(string)) > formatTime(since) {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if sort := sortClause.FindStringSubmatch(request.Query); sort != nil && sort[1] == "MODIFIED_AT_DESC" {
			sorted := append([]map[string]any(nil), rows...)
			for i := range sorted {
				for j := i + 1; j < len(sorted); j++ {
					if sorted[j]["modifiedAt"].(string) > sorted[i]["modifiedAt"].(string) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		} else if sort == nil || sort[1] != "CREATED_AT_ASC" {
			t.Errorf("every query names a sort the fake knows: %s", request.Query)
		}
		page := int(request.Variables["page"].(float64))
		size := int(request.Variables["pageSize"].(float64))
		pages := (len(rows) + size - 1) / size
		from := (page - 1) * size
		if from > len(rows) {
			from = len(rows)
		}
		end := from + size
		if end > len(rows) {
			end = len(rows)
		}
		edges := []any{}
		for _, row := range rows[from:end] {
			edges = append(edges, map[string]any{"node": row})
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"business": map[string]any{"id": "biz1", match[1]: map[string]any{
			"pageInfo": map[string]any{"currentPage": page, "totalPages": pages, "totalCount": len(rows)},
			"edges":    edges,
		}}}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func mustJSON(value any) []byte {
	encoded, _ := json.Marshal(value)
	return encoded
}

func packed(token, business string) string {
	body, _ := json.Marshal(map[string]string{"access_token": token, "business_id": business})
	return string(body)
}

func TestObjectsProveTheTokenAndTheBusiness(t *testing.T) {
	server, calls := fakeWave(t)
	source := New(packed("good", "biz1"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "customers" || objects[1].Label != "Invoices" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.Contains((*calls)[0], `"pageSize":1`) {
		t.Fatalf("objects prove the business with one small query: %v", *calls)
	}
	if live := New(packed("good", "biz1"), ""); live.baseURL != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.baseURL)
	}
	_, err = New(packed("bad", "biz1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "full access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New(packed("good", "biz2"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "no business with that id") {
		t.Fatalf("a business the token cannot read must read as not_connected, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "business id") {
		t.Fatalf("one bare value names the two credentials, got %v", err)
	}
	_, err = source.Describe("bills")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 403 must read as not_connected, got %v", err)
	}
	if err := refusal(http.StatusTooManyRequests, nil); err.(*Failure).Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
	if err := refusal(http.StatusBadGateway, []byte(`{"errors":[{"message":"upstream"}]}`)); err.(*Failure).Code != "source" || !strings.Contains(err.Error(), "upstream") {
		t.Fatalf("another status reads the message, got %v", err)
	}
}

func TestDescribeCountsAndFlattensInvoicesWithMoney(t *testing.T) {
	server, _ := fakeWave(t)
	description, err := New(packed("good", "biz1"), server.URL).Describe("invoices")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || !description.Counted || description.Label != "Invoices" || description.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["customer_id"].Guess != "id" || byName["total"].Guess != "number" || byName["date"].Guess != "date" || byName["customer_email"].Guess != "email" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["total"].Samples; len(got) != 2 || got[0] != "1200.00" || got[1] != "50.00" {
		t.Fatalf("money reads as text or as a number with two places: %v", got)
	}
	if got := byName["amount_due"].Samples; len(got) != 2 || got[0] != "200.00" || got[1] != "0.00" {
		t.Fatalf("amounts due: %v", got)
	}
	if got := byName["status"].Samples; len(got) != 2 || got[0] != "partial" || got[1] != "paid" {
		t.Fatalf("statuses read lowered: %v", got)
	}
	if byName["currency"].Samples[1] != "EUR" || byName["customer_name"].Samples[1] != "Contoso" || byName["customer_email"].Filled != 1 || byName["memo"].Filled != 1 || byName["created"].Samples[0] != "2023-11-01T08:00:00Z" {
		t.Fatalf("currencies, customers, fills and times: %v %v %d %d %v", byName["currency"].Samples, byName["customer_name"].Samples, byName["customer_email"].Filled, byName["memo"].Filled, byName["created"].Samples)
	}
}

func TestPageWalksTheNumberedPagesAndFlattensCustomers(t *testing.T) {
	server, calls := fakeWave(t)
	source := New(packed("good", "biz1"), server.URL)
	first, err := source.Page("customers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["name"] != "Northwind Traders" || row["email_domain"] != "northwind.example" || row["phone"] != "+1 555 010 0000" || row["currency"] != "USD" || row["street"] != "1 Main St" || row["state"] != "Washington" || row["country"] != "US" || row["modified"] != "2023-11-15T11:00:00Z" {
		t.Fatalf("customer row: %v", row)
	}
	if first.Rows[1]["phone"] != "+1 555 010 0001" || first.Rows[1]["state"] != "DE-BE" || first.Rows[1]["country"] != "" {
		t.Fatalf("the phone falls back to the mobile and a province without a name reads its code: %v", first.Rows[1])
	}
	second, err := source.Page("customers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "c3" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "sort: [CREATED_AT_ASC]") && strings.Contains(call, `"page":2`) && strings.Contains(call, `"pageSize":2`) {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not walk the page number in creation order: %v", *calls)
	}
	invoices, err := source.Page("invoices", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(invoices.Rows) != 2 || invoices.Next != nil || invoices.Rows[0]["customer_id"] != "c1" || invoices.Rows[0]["amount_paid"] != "1000.00" || invoices.Rows[0]["view_url"] != "https://wave.example/i1" {
		t.Fatalf("invoice rows: %+v", invoices.Rows)
	}
}

func TestDeltaAsksForTheNewestChangeAfterTheMark(t *testing.T) {
	server, calls := fakeWave(t)
	source := New(packed("good", "biz1"), server.URL)
	delta, err := source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("invoices", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	delta, err = source.Delta("invoices", &Cursor{Offset: 2, Hash: "2023-11-16T08:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("an invoice delta at the mark is unchanged: %+v", delta)
	}
	newest, filtered, unfiltered := 0, 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "sort: [MODIFIED_AT_DESC]") && strings.Contains(call, `"pageSize":1`) {
			newest++
		}
		if strings.Contains(call, "modifiedAtAfter: $since") && strings.Contains(call, `"since":"2023-11-16T08:00:00Z"`) {
			filtered++
		}
		if strings.Contains(call, "customers(") && strings.Contains(call, "modifiedAtAfter") {
			unfiltered++
		}
	}
	if newest != 4 || filtered != 1 || unfiltered != 0 {
		t.Fatalf("each delta is one query newest first, invoices filter by the mark and customers never do: %d %d %d in %v", newest, filtered, unfiltered, *calls)
	}
}
