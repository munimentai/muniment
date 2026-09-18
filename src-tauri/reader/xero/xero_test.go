package xero

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// fakeXero exchanges a refresh token, answers the three endpoints from
// fixtures by page, filters by If-Modified-Since, and refuses a wrong token
// or a tenant the app is not connected to.
func fakeXero(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	stamp := func(value string) string {
		parsed, err := time.Parse(time.RFC3339, value)
		if err != nil {
			t.Fatal(err)
		}
		return "/Date(" + strconv.FormatInt(parsed.UnixMilli(), 10) + "+0000)/"
	}
	contacts := []map[string]any{
		{"ContactID": "c1", "Name": "Northwind Traders", "FirstName": "Ann", "LastName": "Lee", "EmailAddress": "Ann@Northwind.example", "ContactStatus": "ACTIVE", "IsCustomer": true, "IsSupplier": false,
			"Phones":    []any{map[string]any{"PhoneType": "DEFAULT", "PhoneNumber": "0100000", "PhoneAreaCode": "555", "PhoneCountryCode": "1"}, map[string]any{"PhoneType": "MOBILE", "PhoneNumber": ""}},
			"Addresses": []any{map[string]any{"AddressType": "POBOX", "City": "Box City"}, map[string]any{"AddressType": "STREET", "AddressLine1": "1 Main St", "City": "Seattle", "Region": "WA", "PostalCode": "98101", "Country": "US"}},
			"Website":   "https://www.northwind.example/about", "DefaultCurrency": "USD",
			"Balances":       map[string]any{"AccountsReceivable": map[string]any{"Outstanding": 200.5, "Overdue": 0.0}},
			"UpdatedDateUTC": stamp("2023-11-15T11:00:00Z")},
		{"ContactID": "c2", "Name": "Contoso", "ContactStatus": "ACTIVE", "IsCustomer": true, "UpdatedDateUTC": stamp("2023-11-14T11:00:00Z")},
		{"ContactID": "c3", "Name": "Fabrikam", "ContactStatus": "ARCHIVED", "IsCustomer": false, "UpdatedDateUTC": stamp("2023-11-13T11:00:00Z")},
	}
	invoices := []map[string]any{
		{"InvoiceID": "i1", "InvoiceNumber": "INV-0001", "Type": "ACCREC", "Contact": map[string]any{"ContactID": "c1", "Name": "Northwind Traders"}, "Status": "AUTHORISED", "Total": 1200.0, "SubTotal": 1000.0, "TotalTax": 200.0, "AmountDue": 200.0, "AmountPaid": 1000.0,
			"CurrencyCode": "USD", "Date": stamp("2023-11-01T00:00:00Z"), "DateString": "2023-11-01T00:00:00", "DueDate": stamp("2023-12-01T00:00:00Z"), "DueDateString": "2023-12-01T00:00:00", "Reference": "PO 9", "UpdatedDateUTC": stamp("2023-11-16T08:00:00Z")},
		{"InvoiceID": "i2", "InvoiceNumber": "BILL-7", "Type": "ACCPAY", "Contact": map[string]any{"ContactID": "c2", "Name": "Contoso"}, "Status": "PAID", "Total": 50.0, "AmountDue": 0.0, "AmountPaid": 50.0, "CurrencyCode": "EUR",
			"Date": stamp("2023-11-02T00:00:00Z"), "DueDate": stamp("2023-11-20T00:00:00Z"), "FullyPaidOnDate": stamp("2023-11-10T00:00:00Z"), "UpdatedDateUTC": stamp("2023-11-10T00:00:00Z")},
	}
	payments := []map[string]any{
		{"PaymentID": "p1", "Invoice": map[string]any{"InvoiceID": "i1", "InvoiceNumber": "INV-0001", "CurrencyCode": "USD", "Contact": map[string]any{"ContactID": "c1", "Name": "Northwind Traders"}}, "Account": map[string]any{"AccountID": "a1", "Code": "090"},
			"PaymentType": "ACCRECPAYMENT", "Status": "AUTHORISED", "Amount": 1000.0, "CurrencyRate": 1.0, "Date": stamp("2023-11-05T00:00:00Z"), "Reference": "CHK 9", "IsReconciled": true, "UpdatedDateUTC": stamp("2023-11-05T00:00:00Z")},
	}
	data := map[string][]map[string]any{"Contacts": contacts, "Invoices": invoices, "Payments": payments}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery+" since="+r.Header.Get("If-Modified-Since"))
		if r.URL.Path == "/connect/token" {
			_ = r.ParseForm()
			user, pass, _ := r.BasicAuth()
			if user != "app" || pass != "hush" || r.Form.Get("grant_type") != "refresh_token" || r.Form.Get("refresh_token") != "good" {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_grant"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "refresh_token": "next", "token_type": "Bearer"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"Title":"Unauthorized","Status":401,"Detail":"AuthenticationUnsuccessful"}`))
			return
		}
		if r.Header.Get("xero-tenant-id") != "tenant1" {
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"Title":"Forbidden","Status":403,"Detail":"AuthenticationUnsuccessful"}`))
			return
		}
		entity := strings.TrimPrefix(r.URL.Path, "/api.xro/2.0/")
		rows, ok := data[entity]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"Title":"Not Found","Status":404,"Detail":"no such endpoint"}`))
			return
		}
		if since := r.Header.Get("If-Modified-Since"); since != "" {
			floor, err := time.Parse(sinceLayout, since)
			if err != nil {
				t.Errorf("If-Modified-Since reads as UTC without an offset: %s", since)
			}
			kept := []map[string]any{}
			for _, row := range rows {
				if updated, _ := parseTime(row["UpdatedDateUTC"].(string)); updated.After(floor) {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if r.URL.Query().Get("order") == "UpdatedDateUTC DESC" {
			sorted := append([]map[string]any(nil), rows...)
			for i := range sorted {
				for j := i + 1; j < len(sorted); j++ {
					a, _ := parseTime(sorted[i]["UpdatedDateUTC"].(string))
					b, _ := parseTime(sorted[j]["UpdatedDateUTC"].(string))
					if b.After(a) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		page, _ := strconv.Atoi(r.URL.Query().Get("page"))
		size, _ := strconv.Atoi(r.URL.Query().Get("pageSize"))
		if page < 1 || size < 1 {
			t.Errorf("every list names its page and page size: %s", r.URL.RawQuery)
		}
		from := (page - 1) * size
		if from > len(rows) {
			from = len(rows)
		}
		end := from + size
		if end > len(rows) {
			end = len(rows)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"Id": "req", "Status": "OK", entity: rows[from:end]})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(refresh, tenant string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "app", "client_secret": "hush", "refresh_token": refresh, "tenant_id": tenant})
	return string(body)
}

func TestObjectsSignInOnceAndProveTheTenant(t *testing.T) {
	server, calls := fakeXero(t)
	source := New(packed("good", "tenant1"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "contacts" || objects[2].Label != "Payments" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/connect/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if live := New(packed("good", "tenant1"), ""); live.baseURL != DefaultBaseURL || live.tokenURL != DefaultTokenURL {
		t.Fatalf("an empty base URL is the live API and the live token endpoint: %s %s", live.baseURL, live.tokenURL)
	}
	_, err = New(packed("bad", "tenant1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_grant") {
		t.Fatalf("a refused refresh token must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New(packed("good", "tenant2"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "connection to this tenant") {
		t.Fatalf("a tenant the app is not connected to reads as not_connected, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "tenant id") {
		t.Fatalf("one bare value names the four credentials, got %v", err)
	}
	_, err = source.Describe("bills")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a list must read as not_connected, got %v", err)
	}
	if err := refusal(http.StatusTooManyRequests, nil); err.(*Failure).Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
}

func TestDescribeSamplesTheFirstPageAndFlattensInvoicesWithMoney(t *testing.T) {
	server, _ := fakeXero(t)
	description, err := New(packed("good", "tenant1"), server.URL).Describe("invoices")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || description.Counted || description.Label != "Invoices" || description.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["contact_id"].Guess != "id" || byName["total"].Guess != "number" || byName["date"].Guess != "date" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["total"].Samples; len(got) != 2 || got[0] != "1200.00" || got[1] != "50.00" {
		t.Fatalf("totals read as decimals in the major unit: %v", got)
	}
	if got := byName["type"].Samples; len(got) != 2 || got[0] != "receivable" || got[1] != "payable" {
		t.Fatalf("types fold onto words: %v", got)
	}
	if got := byName["date"].Samples; len(got) != 2 || got[0] != "2023-11-01" || got[1] != "2023-11-02" {
		t.Fatalf("dates read from the date string or the stamp: %v", got)
	}
	if byName["currency"].Samples[1] != "EUR" || byName["status"].Samples[0] != "authorised" || byName["fully_paid_date"].Filled != 1 || byName["reference"].Filled != 1 {
		t.Fatalf("currencies, statuses and fills: %v %v %d %d", byName["currency"].Samples, byName["status"].Samples, byName["fully_paid_date"].Filled, byName["reference"].Filled)
	}
	if byName["modified"].Samples[0] != "2023-11-16T08:00:00Z" {
		t.Fatalf("the .NET stamp reads as RFC 3339 in UTC: %v", byName["modified"].Samples)
	}
}

func TestPageWalksTheNumberedPagesAndFlattensContactsAndPayments(t *testing.T) {
	server, calls := fakeXero(t)
	source := New(packed("good", "tenant1"), server.URL)
	first, err := source.Page("contacts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["name"] != "Northwind Traders" || row["email_domain"] != "northwind.example" || row["domain"] != "northwind.example" || row["phone"] != "+1 555 0100000" || row["mobile"] != "" {
		t.Fatalf("contact row: %v", row)
	}
	if row["outstanding"] != "200.50" || row["overdue"] != "0.00" || row["currency"] != "USD" || row["is_customer"] != "yes" || row["is_supplier"] != "no" || row["status"] != "active" {
		t.Fatalf("contact balances and flags: %v", row)
	}
	if row["street"] != "1 Main St" || row["city"] != "Seattle" || row["state"] != "WA" || row["postal_code"] != "98101" || row["country"] != "US" {
		t.Fatalf("the street address wins over the postal one: %v", row)
	}
	second, err := source.Page("contacts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "c3" || second.Rows[0]["status"] != "archived" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "/api.xro/2.0/Contacts?page=2&pageSize=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not walk the page number: %v", *calls)
	}
	payments, err := source.Page("payments", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := payments.Rows[0]
	if item["invoice_id"] != "i1" || item["contact_id"] != "c1" || item["account_id"] != "a1" || item["amount"] != "1000.00" || item["currency"] != "USD" || item["type"] != "accrecpayment" || item["reconciled"] != "yes" || item["date"] != "2023-11-05" {
		t.Fatalf("payment row: %v", item)
	}
	if payments.Next != nil {
		t.Fatalf("a short page ends the walk: %+v", payments.Next)
	}
}

func TestDeltaAsksOnceAfterTheMark(t *testing.T) {
	server, calls := fakeXero(t)
	source := New(packed("good", "tenant1"), server.URL)
	delta, err := source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00Z"})
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
	queries, marked := 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "order=UpdatedDateUTC+DESC&page=1&pageSize=1") {
			queries++
		}
		if strings.Contains(call, "since=2023-11-14T00:00:00") {
			marked++
		}
	}
	if queries != 3 || marked != 1 {
		t.Fatalf("each delta is one query and the mark travels in If-Modified-Since: %d %d in %v", queries, marked, *calls)
	}
}
