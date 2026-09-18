package freshbooks

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeFreshBooks exchanges a refresh token, answers the accounting lists
// in numbered pages under one account, filters by updated_since, and
// refuses a wrong token.
func fakeFreshBooks(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	clients := []map[string]any{
		{"id": 101.0, "fname": "Ann", "lname": "Lee", "organization": "Northwind Traders", "email": "Ann@Northwind.example", "bus_phone": "", "mob_phone": "+1 555 010 0000",
			"p_city": "Seattle", "p_province": "WA", "p_country": "United States", "currency_code": "USD", "vis_state": 0.0, "signup_date": "2023-01-01 09:00:00", "updated": "2023-11-15 11:00:00"},
		{"id": 102.0, "fname": "Bo", "lname": "Chan", "organization": "", "email": "bo@contoso.example", "vis_state": 2.0, "signup_date": "2023-02-01 09:00:00", "updated": "2023-11-14 11:00:00"},
		{"id": 103.0, "organization": "Fabrikam", "vis_state": 0.0, "signup_date": "2023-03-01 09:00:00", "updated": "2023-11-13 11:00:00"},
	}
	invoices := []map[string]any{
		{"id": 501.0, "invoice_number": "0000001", "customerid": 101.0, "organization": "Northwind Traders", "v3_status": "partial", "amount": map[string]any{"amount": "1200.00", "code": "USD"},
			"outstanding": map[string]any{"amount": "200.00", "code": "USD"}, "paid": map[string]any{"amount": "1000.00", "code": "USD"}, "currency_code": "USD", "create_date": "2023-11-01", "due_date": "2023-12-01",
			"po_number": "PO-9", "created_at": "2023-11-01 08:00:00", "updated": "2023-11-16 08:00:00"},
		{"id": 502.0, "invoice_number": "0000002", "customerid": 102.0, "fname": "Bo", "lname": "Chan", "v3_status": "paid", "amount": map[string]any{"amount": "50.00", "code": "EUR"}, "date_paid": "2023-11-10", "updated": "2023-11-10 08:00:00"},
	}
	payments := []map[string]any{
		{"id": 901.0, "invoiceid": 501.0, "clientid": 101.0, "amount": map[string]any{"amount": "1000.00", "code": "USD"}, "date": "2023-11-05", "type": "Check", "note": "first half", "vis_state": 0.0, "updated": "2023-11-05 08:00:00"},
	}
	data := map[string][]map[string]any{"users/clients": clients, "invoices/invoices": invoices, "payments/payments": payments}
	keys := map[string]string{"users/clients": "clients", "invoices/invoices": "invoices", "payments/payments": "payments"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/auth/oauth/token" {
			var grant map[string]string
			_ = json.NewDecoder(r.Body).Decode(&grant)
			if grant["grant_type"] != "refresh_token" || grant["client_id"] != "app" || grant["client_secret"] != "hush" || grant["refresh_token"] != "good" {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"invalid_grant","error_description":"The provided authorization grant is invalid"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "refresh_token": "next", "token_type": "Bearer"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"response":{"errors":[{"errno":1003,"field":"access_token","message":"The server could not verify that you are authorized."}]}}`))
			return
		}
		const prefix = "/accounting/account/acc1/"
		if !strings.HasPrefix(r.URL.Path, prefix) {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"response":{"errors":[{"message":"no such account"}]}}`))
			return
		}
		endpoint := strings.TrimPrefix(r.URL.Path, prefix)
		rows, ok := data[endpoint]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		if since := r.URL.Query().Get("search[updated_since]"); since != "" {
			kept := []map[string]any{}
			for _, row := range rows {
				if row["updated"].(string) > since {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		per, _ := strconv.Atoi(r.URL.Query().Get("per_page"))
		if per <= 0 {
			per = 15
		}
		page, _ := strconv.Atoi(r.URL.Query().Get("page"))
		if page <= 0 {
			page = 1
		}
		start := (page - 1) * per
		if start > len(rows) {
			start = len(rows)
		}
		end := start + per
		if end > len(rows) {
			end = len(rows)
		}
		pages := (len(rows) + per - 1) / per
		result := map[string]any{keys[endpoint]: rows[start:end], "page": page, "pages": pages, "per_page": per, "total": len(rows)}
		_ = json.NewEncoder(w).Encode(map[string]any{"response": map[string]any{"result": result}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(refresh, account string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "app", "client_secret": "hush", "refresh_token": refresh, "account_id": account})
	return string(body)
}

func TestObjectsSignInOnceAndProveTheAccount(t *testing.T) {
	server, calls := fakeFreshBooks(t)
	source := New(packed("good", "acc1"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "clients" || objects[2].Label != "Payments" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/auth/oauth/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if !strings.Contains((*calls)[1], "/accounting/account/acc1/users/clients?page=1&per_page=1") {
		t.Fatalf("the account was not proved with one small call: %v", *calls)
	}
	if live := New(packed("good", "acc1"), ""); live.baseURL != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.baseURL)
	}
	_, err = New(packed("bad", "acc1"), server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_grant") {
		t.Fatalf("a refused refresh token must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New(packed("good", "acc2"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "source" || !strings.Contains(failure.Message, "no such account") {
		t.Fatalf("a wrong account id reads FreshBooks' message, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "account id") {
		t.Fatalf("one bare value names the four credentials, got %v", err)
	}
	_, err = source.Describe("expenses")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal("clients", http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a list must read as not_connected, got %v", err)
	}
}

func TestDescribeCountsAndFlattensInvoicesWithMoney(t *testing.T) {
	server, _ := fakeFreshBooks(t)
	description, err := New(packed("good", "acc1"), server.URL).Describe("invoices")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || !description.Counted || description.Label != "Invoices" || description.Hash != "2023-11-16 08:00:00" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["client_id"].Guess != "id" || byName["amount"].Guess != "number" || byName["currency"].Guess != "string" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["amount"].Samples; len(got) != 2 || got[0] != "1200.00" || got[1] != "50.00" {
		t.Fatalf("amounts read as decimals in the major unit: %v", got)
	}
	if got := byName["currency"].Samples; len(got) != 2 || got[0] != "USD" || got[1] != "EUR" {
		t.Fatalf("currencies read from the amount: %v", got)
	}
	if byName["client_name"].Samples[1] != "Bo Chan" || byName["created"].Samples[0] != "2023-11-01T08:00:00" || byName["paid_at"].Filled != 1 {
		t.Fatalf("names, times and fills: %v %v %d", byName["client_name"].Samples, byName["created"].Samples, byName["paid_at"].Filled)
	}
}

func TestPageWalksNumberedPagesAndFlattensClientsAndPayments(t *testing.T) {
	server, calls := fakeFreshBooks(t)
	source := New(packed("good", "acc1"), server.URL)
	first, err := source.Page("clients", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15 11:00:00" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["name"] != "Northwind Traders" || row["email_domain"] != "northwind.example" || row["phone"] != "+1 555 010 0000" || row["status"] != "active" || row["created"] != "2023-01-01T09:00:00" {
		t.Fatalf("client row: %v", row)
	}
	if first.Rows[1]["name"] != "Bo Chan" || first.Rows[1]["status"] != "archived" {
		t.Fatalf("a client without an organization is named by the person: %v", first.Rows[1])
	}
	second, err := source.Page("clients", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "103" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15 11:00:00" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "/users/clients?page=2&per_page=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not ask for page 2: %v", *calls)
	}
	payments, err := source.Page("payments", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := payments.Rows[0]
	if item["invoice_id"] != "501" || item["client_id"] != "101" || item["amount"] != "1000.00" || item["currency"] != "USD" || item["type"] != "Check" || item["date"] != "2023-11-05" {
		t.Fatalf("payment row: %v", item)
	}
}

func TestDeltaQueriesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeFreshBooks(t)
	source := New(packed("good", "acc1"), server.URL)
	delta, err := source.Delta("clients", &Cursor{Offset: 3, Hash: "2023-11-15 11:00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("clients", &Cursor{Offset: 3, Hash: "2023-11-14 00:00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15 11:00:00" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("invoices", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16 08:00:00" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.Contains(call, "search%5Bupdated_since%5D=2023-11-1") {
			filtered++
		}
	}
	if filtered != 2 {
		t.Fatalf("each delta with a mark is one filtered query: %d in %v", filtered, *calls)
	}
}
