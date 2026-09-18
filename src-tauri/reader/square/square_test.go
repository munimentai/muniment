package square

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeSquare answers the locations list, the four object lists and the
// three change searches from fixtures, pages by an index cursor, and
// refuses a wrong token.
func fakeSquare(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	customers := []map[string]any{
		{"id": "cus_1", "given_name": "Ann", "family_name": "Lee", "company_name": "Northwind Traders", "email_address": "Ann@Northwind.example", "phone_number": "+1 555 010 0000",
			"address":      map[string]any{"address_line_1": "1 Pier", "locality": "Seattle", "administrative_district_level_1": "WA", "postal_code": "98101", "country": "US"},
			"reference_id": "NW-1", "creation_source": "THIRD_PARTY", "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-16T09:00:00Z"},
		{"id": "cus_2", "company_name": "Contoso", "created_at": "2023-11-14T22:15:00Z", "updated_at": "2023-11-15T09:00:00Z"},
		{"id": "cus_3", "given_name": "Bo", "email_address": "bo@fabrikam.example", "created_at": "2023-11-13T22:13:20Z", "updated_at": "2023-11-14T09:00:00Z"},
	}
	orders := []map[string]any{
		{"id": "ord_1", "location_id": "loc_1", "customer_id": "cus_1", "state": "COMPLETED", "reference_id": "R-1", "source": map[string]any{"name": "Online Store"},
			"total_money": map[string]any{"amount": 15700.0, "currency": "USD"}, "total_tax_money": map[string]any{"amount": 1200.0, "currency": "USD"},
			"total_discount_money": map[string]any{"amount": 0.0, "currency": "USD"}, "total_tip_money": map[string]any{"amount": 500.0, "currency": "USD"},
			"net_amount_due_money": map[string]any{"amount": 0.0, "currency": "USD"}, "line_items": []any{map[string]any{"name": "Tea"}, map[string]any{"name": "Cup"}},
			"created_at": "2023-11-14T22:20:00Z", "closed_at": "2023-11-14T22:25:00Z", "updated_at": "2023-11-14T22:25:00Z"},
		{"id": "ord_2", "location_id": "loc_2", "state": "CANCELED", "total_money": map[string]any{"amount": 5000.0, "currency": "JPY"}, "created_at": "2023-11-15T22:20:00Z", "updated_at": "2023-11-15T22:25:00Z"},
	}
	payments := []map[string]any{
		{"id": "pay_1", "order_id": "ord_1", "customer_id": "cus_1", "location_id": "loc_1", "status": "COMPLETED", "source_type": "CARD",
			"amount_money": map[string]any{"amount": 15200.0, "currency": "USD"}, "tip_money": map[string]any{"amount": 500.0, "currency": "USD"}, "total_money": map[string]any{"amount": 15700.0, "currency": "USD"},
			"card_details": map[string]any{"card": map[string]any{"card_brand": "VISA", "last_4": "4242"}}, "receipt_number": "AbCd", "receipt_url": "https://squareup.example/receipt/AbCd",
			"created_at": "2023-11-14T22:21:00Z", "updated_at": "2023-11-14T22:26:00Z"},
	}
	invoices := []map[string]any{
		{"id": "inv_1", "invoice_number": "0001", "title": "November", "status": "PARTIALLY_PAID", "location_id": "loc_1", "order_id": "ord_1",
			"primary_recipient": map[string]any{"customer_id": "cus_1", "given_name": "Ann", "family_name": "Lee", "email_address": "ann@northwind.example"},
			"payment_requests": []any{
				map[string]any{"request_type": "DEPOSIT", "due_date": "2023-11-20", "computed_amount_money": map[string]any{"amount": 5000.0, "currency": "USD"}, "total_completed_amount_money": map[string]any{"amount": 5000.0, "currency": "USD"}},
				map[string]any{"request_type": "BALANCE", "due_date": "2023-12-14", "computed_amount_money": map[string]any{"amount": 10700.0, "currency": "USD"}, "total_completed_amount_money": map[string]any{"amount": 0.0, "currency": "USD"}},
			},
			"public_url": "https://squareup.example/pay-invoice/inv_1", "sale_or_service_date": "2023-11-14", "created_at": "2023-11-14T23:00:00Z", "updated_at": "2023-11-20T09:00:00Z"},
	}
	send := func(w http.ResponseWriter, key string, rows []map[string]any, limit, start int) {
		end := start + limit
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{key: rows[start:end]}
		if end < len(rows) {
			answer["cursor"] = strconv.Itoa(end)
		}
		_ = json.NewEncoder(w).Encode(answer)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery+" "+string(raw))
		if r.Header.Get("Authorization") != "Bearer sq_good" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"errors":[{"category":"AUTHENTICATION_ERROR","code":"UNAUTHORIZED","detail":"This request could not be authorized."}]}`))
			return
		}
		if r.Header.Get("Square-Version") == "" {
			t.Errorf("no Square-Version header on %s", r.URL.Path)
		}
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		limit := 100
		if text := r.URL.Query().Get("limit"); text != "" {
			limit, _ = strconv.Atoi(text)
		}
		if value, ok := body["limit"].(float64); ok {
			limit = int(value)
		}
		start, _ := strconv.Atoi(r.URL.Query().Get("cursor"))
		if value, ok := body["cursor"].(string); ok {
			start, _ = strconv.Atoi(value)
		}
		switch r.Method + " " + r.URL.Path {
		case "GET /v2/locations":
			_ = json.NewEncoder(w).Encode(map[string]any{"locations": []any{map[string]any{"id": "loc_1", "name": "Pier"}, map[string]any{"id": "loc_2", "name": "Market"}}})
		case "GET /v2/customers":
			send(w, "customers", customers, limit, start)
		case "POST /v2/customers/search":
			since := ""
			if query, ok := body["query"].(map[string]any); ok {
				since = query["filter"].(map[string]any)["updated_at"].(map[string]any)["start_at"].(string)
			}
			results := []map[string]any{}
			for _, row := range customers {
				if row["updated_at"].(string) >= since {
					results = append(results, row)
				}
			}
			send(w, "customers", results, limit, 0)
		case "POST /v2/orders/search":
			locations, _ := body["location_ids"].([]any)
			if len(locations) != 2 {
				t.Errorf("orders searched without both locations: %s", raw)
			}
			results := orders
			if query, ok := body["query"].(map[string]any); ok {
				since := ""
				if filter, ok := query["filter"].(map[string]any); ok {
					since = filter["date_time_filter"].(map[string]any)["updated_at"].(map[string]any)["start_at"].(string)
				}
				results = []map[string]any{}
				for _, row := range orders {
					if row["updated_at"].(string) >= since {
						results = append(results, row)
					}
				}
				for i := range results {
					for j := i + 1; j < len(results); j++ {
						if results[j]["updated_at"].(string) > results[i]["updated_at"].(string) {
							results[i], results[j] = results[j], results[i]
						}
					}
				}
			}
			send(w, "orders", results, limit, start)
		case "GET /v2/payments":
			results := payments
			if since := r.URL.Query().Get("updated_at_begin_time"); since != "" {
				if r.URL.Query().Get("sort_field") != "UPDATED_AT" {
					t.Errorf("payments filtered by update without the matching sort: %s", r.URL.RawQuery)
				}
				results = []map[string]any{}
				for _, row := range payments {
					if row["updated_at"].(string) >= since {
						results = append(results, row)
					}
				}
			}
			send(w, "payments", results, limit, start)
		case "POST /v2/invoices/search":
			query, _ := body["query"].(map[string]any)
			filter, _ := query["filter"].(map[string]any)
			if locations, _ := filter["location_ids"].([]any); len(locations) != 2 {
				t.Errorf("invoices searched without both locations: %s", raw)
			}
			send(w, "invoices", invoices, limit, start)
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"errors":[{"category":"INVALID_REQUEST_ERROR","code":"NOT_FOUND","detail":"Not found"}]}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheTokenAndListFour(t *testing.T) {
	server, calls := fakeSquare(t)
	source := New("sq_good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "customers" || objects[3].Label != "Invoices" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "GET /v2/locations") {
		t.Fatalf("the token was not proved with the locations list: %v", *calls)
	}
	if New("", server.URL).baseURL != server.URL || New("x", "").baseURL != DefaultBaseURL {
		t.Fatal("an empty base URL is the production API")
	}

	_, err = New("sq_bad", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New("", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("an empty token must read as not_connected, got %v", err)
	}
	_, err = source.Describe("refunds")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	_, err = New("sq_bad", server.URL).Page("customers", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("a refused page must read as not_connected, got %v", err)
	}
}

func TestDescribeFlattensCustomersWithSamplesAndGuesses(t *testing.T) {
	server, _ := fakeSquare(t)
	description, err := New("sq_good", server.URL).Describe("customers")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Customers" || description.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["created"].Guess != "date-time" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email_domain"].Samples; len(got) != 2 || got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["email"].Filled != 2 || byName["name"].Filled != 2 || byName["company_name"].Filled != 2 {
		t.Fatalf("filled counts: %+v", byName)
	}
	if got := byName["name"].Samples; got[0] != "Ann Lee" || got[1] != "Bo" {
		t.Fatalf("names: %v", got)
	}
	if got := byName["address_city"].Samples; len(got) != 1 || got[0] != "Seattle" {
		t.Fatalf("address: %v", got)
	}
	if got := byName["creation_source"].Samples; got[0] != "third_party" {
		t.Fatalf("creation source: %v", got)
	}
}

func TestPageWalksTheCursorAndFlattensMoney(t *testing.T) {
	server, calls := fakeSquare(t)
	source := New("sq_good", server.URL)
	first, err := source.Page("customers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("customers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "cus_3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("the mark keeps the newest update seen: %s", second.Hash)
	}
	if !strings.Contains((*calls)[1], "cursor=2") || !strings.Contains((*calls)[1], "limit=2") {
		t.Fatalf("the second page did not carry the cursor: %v", *calls)
	}

	orders, err := source.Page("orders", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(orders.Rows) != 2 {
		t.Fatalf("orders: %+v", orders)
	}
	row := orders.Rows[0]
	if row["customer_id"] != "cus_1" || row["location_id"] != "loc_1" || row["status"] != "completed" || row["source"] != "Online Store" {
		t.Fatalf("order row: %v", row)
	}
	if row["total"] != "157.00" || row["tax"] != "12.00" || row["tip"] != "5.00" || row["amount_due"] != "0.00" || row["currency"] != "USD" || row["line_items"] != "2" {
		t.Fatalf("order money: %v", row)
	}
	if row["closed_at"] != "2023-11-14T22:25:00Z" {
		t.Fatalf("order times: %v", row)
	}
	if yen := orders.Rows[1]; yen["total"] != "5000" || yen["currency"] != "JPY" || yen["status"] != "cancelled" || yen["customer_id"] != "" {
		t.Fatalf("a zero decimal order: %v", yen)
	}

	payments, err := source.Page("payments", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	payment := payments.Rows[0]
	if payment["order_id"] != "ord_1" || payment["customer_id"] != "cus_1" || payment["status"] != "completed" || payment["amount"] != "152.00" || payment["total"] != "157.00" {
		t.Fatalf("payment row: %v", payment)
	}
	if payment["card_brand"] != "VISA" || payment["card_last_4"] != "4242" || payment["source_type"] != "card" || payment["refunded"] != "" {
		t.Fatalf("payment card: %v", payment)
	}

	invoices, err := source.Page("invoices", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	invoice := invoices.Rows[0]
	if invoice["customer_id"] != "cus_1" || invoice["order_id"] != "ord_1" || invoice["customer_name"] != "Ann Lee" || invoice["status"] != "partially_paid" {
		t.Fatalf("invoice row: %v", invoice)
	}
	if invoice["total"] != "157.00" || invoice["paid"] != "50.00" || invoice["currency"] != "USD" || invoice["due_date"] != "2023-12-14" {
		t.Fatalf("invoice money: %v", invoice)
	}
	if invoices.Hash != "2023-11-20T09:00:00Z" {
		t.Fatalf("invoice mark: %s", invoices.Hash)
	}
}

func TestDeltaFiltersByUpdateTime(t *testing.T) {
	server, calls := fakeSquare(t)
	source := New("sq_good", server.URL)
	delta, err := source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-16T09:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	if !strings.Contains((*calls)[0], `"start_at":"2023-11-16T09:00:00Z"`) {
		t.Fatalf("the customer search did not filter by the mark: %v", *calls)
	}
	delta, err = source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-15T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("customers", nil)
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("a first delta: %+v %v", delta, err)
	}

	delta, err = source.Delta("orders", &Cursor{Hash: "2023-11-15T22:25:00Z"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("orders delta: %+v %v", delta, err)
	}
	last := (*calls)[len(*calls)-1]
	if !strings.Contains(last, `"sort_field":"UPDATED_AT"`) || !strings.Contains(last, `"limit":1`) {
		t.Fatalf("the order search did not sort by update: %s", last)
	}
	delta, err = source.Delta("orders", &Cursor{Hash: "2023-11-14T00:00:00Z"})
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-15T22:25:00Z" {
		t.Fatalf("orders delta: %+v %v", delta, err)
	}

	delta, err = source.Delta("payments", &Cursor{Hash: "2023-11-14T00:00:00Z"})
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-14T22:26:00Z" {
		t.Fatalf("payments delta: %+v %v", delta, err)
	}
	if last := (*calls)[len(*calls)-1]; !strings.Contains(last, "updated_at_begin_time=2023-11-14T00%3A00%3A00Z") || !strings.Contains(last, "limit=1") {
		t.Fatalf("the payments list did not filter by the mark: %s", last)
	}

	delta, err = source.Delta("invoices", &Cursor{Hash: "2023-11-20T09:00:00Z"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("invoices delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("invoices", &Cursor{Hash: "2023-11-01T00:00:00Z"})
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-20T09:00:00Z" {
		t.Fatalf("invoices delta: %+v %v", delta, err)
	}
}

func TestAmountsAndMarks(t *testing.T) {
	if formatAmount(1999, "USD") != "19.99" || formatAmount(1999, "jpy") != "1999" || formatAmount(-50, "EUR") != "-0.50" {
		t.Fatal("minor units did not read by currency")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("an empty list keeps the mark")
	}
	if newestUpdated([]record{{"updated_at": "2022-01-01T00:00:00Z"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
	if newestUpdated([]record{{"updated_at": "2023-01-01T05:00:00+05:00"}}, "") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark reads in UTC")
	}
}
