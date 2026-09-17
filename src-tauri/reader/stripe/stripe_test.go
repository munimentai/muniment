package stripe

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// fakeStripe answers the three list endpoints from fixtures, pages customers
// by starting_after, and refuses a wrong key.
func fakeStripe(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	customers := []map[string]any{
		{"id": "cus_1", "object": "customer", "name": "Northwind Traders", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000",
			"created": 1700000000.0, "currency": "usd", "balance": -1250.0, "delinquent": false, "livemode": true,
			"address":  map[string]any{"line1": "1 Pier", "city": "Seattle", "state": "WA", "postal_code": "98101", "country": "US"},
			"metadata": map[string]any{"tier": "gold", "seats": 12.0}},
		{"id": "cus_2", "object": "customer", "name": "Contoso", "email": nil, "created": 1700000100.0, "currency": "jpy", "balance": 5000.0, "delinquent": true, "metadata": map[string]any{}},
		{"id": "cus_3", "object": "customer", "name": "Fabrikam", "email": "ops@fabrikam.example", "created": 1699999000.0, "metadata": map[string]any{}},
	}
	subscriptions := []map[string]any{
		{"id": "sub_1", "customer": "cus_1", "status": "trialing", "currency": "usd", "created": 1700000200.0, "start_date": 1700000200.0,
			"current_period_start": 1700000200.0, "current_period_end": 1702592200.0, "cancel_at_period_end": false, "livemode": true,
			"items": map[string]any{"data": []any{
				map[string]any{"quantity": 3.0, "price": map[string]any{"id": "price_1", "nickname": "Team", "unit_amount": 4900.0, "recurring": map[string]any{"interval": "month", "interval_count": 1.0}}},
				map[string]any{"quantity": 1.0, "price": map[string]any{"id": "price_2", "unit_amount": 1000.0, "product": "prod_addon", "recurring": map[string]any{"interval": "month", "interval_count": 1.0}}},
			}}},
		{"id": "sub_2", "customer": map[string]any{"id": "cus_2"}, "status": "canceled", "currency": "usd", "created": 1690000000.0, "canceled_at": 1695000000.0,
			"items": map[string]any{"data": []any{
				map[string]any{"price": map[string]any{"id": "price_3", "unit_amount": 120000.0, "product": map[string]any{"id": "prod_9", "name": "Enterprise"}, "recurring": map[string]any{"interval": "year", "interval_count": 1.0}}},
			}}},
	}
	invoices := []map[string]any{
		{"id": "in_1", "number": "A-0001", "customer": "cus_1", "customer_email": "ann@northwind.example", "customer_name": "Northwind Traders", "status": "paid",
			"amount_due": 15700.0, "amount_paid": 15700.0, "total": 15700.0, "currency": "usd", "created": 1700000300.0, "due_date": 1702592300.0,
			"status_transitions": map[string]any{"paid_at": 1700000400.0}, "hosted_invoice_url": "https://invoice.example/in_1", "livemode": true},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		if r.Header.Get("Authorization") != "Bearer sk_test_good" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":{"message":"Invalid API Key provided"}}`))
			return
		}
		var data []map[string]any
		switch r.URL.Path {
		case "/v1/customers":
			data = customers
		case "/v1/subscriptions":
			if r.URL.Query().Get("status") != "all" {
				t.Errorf("subscriptions listed without status=all: %s", r.URL.RawQuery)
			}
			if r.URL.Query().Get("expand[]") != "data.items.data.price.product" {
				t.Errorf("subscriptions listed without the product expanded: %s", r.URL.RawQuery)
			}
			data = subscriptions
		case "/v1/invoices":
			data = invoices
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":{"message":"Unrecognized request URL"}}`))
			return
		}
		limit := 100
		if text := r.URL.Query().Get("limit"); text != "" {
			_, _ = json.Number(text).Int64()
			limit = int(mustInt(t, text))
		}
		start := 0
		if after := r.URL.Query().Get("starting_after"); after != "" {
			for index, item := range data {
				if item["id"] == after {
					start = index + 1
				}
			}
		}
		end := start + limit
		if end > len(data) {
			end = len(data)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"object": "list", "data": data[start:end], "has_more": end < len(data)})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func mustInt(t *testing.T, text string) int64 {
	t.Helper()
	value, err := json.Number(text).Int64()
	if err != nil {
		t.Fatalf("%q is not a number", text)
	}
	return value
}

func TestObjectsProveTheKeyAndListThree(t *testing.T) {
	server, calls := fakeStripe(t)
	source := New("sk_test_good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "customers" || objects[2].Label != "Invoices" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "/v1/customers?limit=1") {
		t.Fatalf("the key was not proved with one small call: %v", *calls)
	}

	_, err = New("sk_test_bad", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = source.Describe("charges")
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestDescribeFlattensCustomersWithSamplesAndGuesses(t *testing.T) {
	server, _ := fakeStripe(t)
	description, err := New("sk_test_good", server.URL).Describe("customers")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Customers" || description.Hash != "1700000100" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email_domain"].Samples; len(got) != 2 || got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["email"].Filled != 2 || byName["name"].Filled != 3 {
		t.Fatalf("filled counts: email %d name %d", byName["email"].Filled, byName["name"].Filled)
	}
	if got := byName["created"].Samples[0]; got != "2023-11-14T22:13:20Z" {
		t.Fatalf("created: %s", got)
	}
	if got := byName["balance"].Samples; got[0] != "-12.50" || got[1] != "5000" {
		t.Fatalf("balance reads minor units by currency: %v", got)
	}
	if got := byName["delinquent"].Samples; got[0] != "no" || got[1] != "yes" {
		t.Fatalf("delinquent: %v", got)
	}
	if got := byName["address_city"].Samples; len(got) != 1 || got[0] != "Seattle" {
		t.Fatalf("address: %v", got)
	}
}

func TestPageWalksStartingAfterAndFlattensSubscriptions(t *testing.T) {
	server, calls := fakeStripe(t)
	source := New("sk_test_good", server.URL)
	first, err := source.Page("customers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "cus_2" || first.Next.Offset != 2 || first.Hash != "1700000100" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	if first.Rows[0]["metadata_tier"] != "gold" || first.Rows[0]["metadata_seats"] != "12" {
		t.Fatalf("metadata columns: %v", first.Rows[0])
	}
	second, err := source.Page("customers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "cus_3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "1700000100" {
		t.Fatalf("the mark keeps the newest creation seen: %s", second.Hash)
	}
	if !strings.Contains((*calls)[1], "starting_after=cus_2") {
		t.Fatalf("the second page did not start after cus_2: %v", *calls)
	}

	subscriptions, err := source.Page("subscriptions", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := subscriptions.Rows[0]
	if row["customer"] != "cus_1" || row["state"] != "trial" || row["plan"] != "Team" || row["billing_period"] != "month" || row["amount"] != "157.00" {
		t.Fatalf("subscription row: %v", row)
	}
	if row["started_at"] != "2023-11-14T22:16:40Z" || row["cancel_at_period_end"] != "no" || row["cancelled_at"] != "" {
		t.Fatalf("subscription times: %v", row)
	}
	second_ := subscriptions.Rows[1]
	if second_["customer"] != "cus_2" || second_["state"] != "cancelled" || second_["plan"] != "Enterprise" || second_["billing_period"] != "year" || second_["amount"] != "1200.00" {
		t.Fatalf("cancelled subscription row: %v", second_)
	}
	if second_["cancelled_at"] != "2023-09-18T01:20:00Z" {
		t.Fatalf("cancelled_at: %s", second_["cancelled_at"])
	}

	invoices, err := source.Page("invoices", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	invoice := invoices.Rows[0]
	if invoice["number"] != "A-0001" || invoice["status"] != "paid" || invoice["amount_due"] != "157.00" || invoice["paid_at"] != "2023-11-14T22:20:00Z" {
		t.Fatalf("invoice row: %v", invoice)
	}
}

func TestDeltaReadsTheNewestCreation(t *testing.T) {
	server, _ := fakeStripe(t)
	source := New("sk_test_good", server.URL)
	delta, err := source.Delta("customers", &Cursor{Offset: 3, Hash: "1700000100"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("customers", &Cursor{Offset: 3, Hash: "1600000000"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "1700000100" {
		t.Fatalf("delta: %+v", delta)
	}
}

func TestAmountsFollowTheCurrency(t *testing.T) {
	if formatAmount(1999, "usd") != "19.99" || formatAmount(1999, "JPY") != "1999" || formatAmount(-50, "eur") != "-0.50" {
		t.Fatal("minor units did not read by currency")
	}
	if scalar(3.0) != "3" || scalar(2.5) != "2.5" || scalar(true) != "yes" || scalar(nil) != "" {
		t.Fatal("scalars did not read as text")
	}
	if newestCreated(nil, "42") != "42" || newestCreated([]map[string]any{{"created": 7.0}}, "42") != "42" {
		t.Fatal("the mark never moves backwards")
	}
}
