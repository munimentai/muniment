package paypal

import (
	"encoding/json"
	"math"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// fakePayPal grants a token for one app, answers the transaction report by
// window and page, answers the invoice list by page with its total, and
// refuses wrong credentials and a wrong token.
func fakePayPal(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	money := func(value string) map[string]any { return map[string]any{"currency_code": "USD", "value": value} }
	transactions := []map[string]any{
		{"transaction_info": map[string]any{"transaction_id": "T1", "transaction_event_code": "T0006", "transaction_status": "S", "transaction_amount": money("120.00"), "fee_amount": money("-3.78"), "transaction_subject": "Order 9", "invoice_id": "INV-1",
			"paypal_reference_id": "O-1", "paypal_reference_id_type": "ODR", "custom_field": "web", "transaction_initiation_date": "2023-11-01T10:00:00+0000", "transaction_updated_date": "2023-11-15T11:00:00+0000"},
			"payer_info": map[string]any{"account_id": "P1", "email_address": "Ann@Northwind.example", "country_code": "US", "payer_name": map[string]any{"given_name": "Ann", "surname": "Lee", "alternate_full_name": "Ann Lee"}}},
		{"transaction_info": map[string]any{"transaction_id": "T2", "transaction_event_code": "T0006", "transaction_status": "P", "transaction_amount": money("50.00"), "transaction_initiation_date": "2023-11-05T10:00:00+0000", "transaction_updated_date": "2023-11-14T11:00:00+0000"},
			"payer_info": map[string]any{"account_id": "P2", "email_address": "bob@contoso.example", "payer_name": map[string]any{"given_name": "Bob", "surname": "Ray"}}},
		{"transaction_info": map[string]any{"transaction_id": "T3", "transaction_event_code": "T1107", "transaction_status": "V", "transaction_amount": money("-50.00"), "transaction_initiation_date": "2023-11-10T10:00:00+0000", "transaction_updated_date": "2023-11-13T11:00:00+0000"}},
	}
	invoices := []map[string]any{
		{"id": "INV2-1", "status": "SENT", "detail": map[string]any{"invoice_number": "0001", "reference": "PO 9", "currency_code": "USD", "invoice_date": "2023-11-01", "note": "Thank you", "memo": "Net 30", "payment_term": map[string]any{"term_type": "NET_30", "due_date": "2023-12-01"},
			"metadata": map[string]any{"create_time": "2023-11-01T08:00:00Z", "last_update_time": "2023-11-16T08:00:00Z", "recipient_view_url": "https://www.paypal.com/invoice/p/INV2-1"}},
			"amount": money("1200.00"), "due_amount": money("200.00"), "payments": map[string]any{"paid_amount": money("1000.00")},
			"primary_recipients": []any{map[string]any{"billing_info": map[string]any{"name": map[string]any{"given_name": "Ann", "surname": "Lee", "full_name": "Ann Lee"}, "business_name": "Northwind Traders", "email_address": "ap@northwind.example"}}}},
		{"id": "INV2-2", "status": "PAID", "detail": map[string]any{"invoice_number": "0002", "currency_code": "EUR", "invoice_date": "2023-11-02", "metadata": map[string]any{"create_time": "2023-11-02T08:00:00Z", "last_update_time": "2023-11-10T08:00:00Z"}},
			"amount": map[string]any{"currency_code": "EUR", "value": "50.00"}, "due_amount": map[string]any{"currency_code": "EUR", "value": "0.00"}, "payments": map[string]any{"paid_amount": map[string]any{"currency_code": "EUR", "value": "50.00"}},
			"primary_recipients": []any{map[string]any{"billing_info": map[string]any{"name": map[string]any{"given_name": "Bob", "surname": "Ray"}, "email_address": "bob@contoso.example"}}}},
		{"id": "INV2-3", "status": "DRAFT", "detail": map[string]any{"invoice_number": "0003", "currency_code": "USD", "invoice_date": "2023-11-03", "metadata": map[string]any{"create_time": "2023-11-03T08:00:00Z", "last_update_time": "2023-11-03T08:00:00Z"}}, "amount": money("10.00")},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/v1/oauth2/token" {
			_ = r.ParseForm()
			user, pass, _ := r.BasicAuth()
			if user != "app" || pass != "hush" || r.Form.Get("grant_type") != "client_credentials" {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"invalid_client","error_description":"Client Authentication failed"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "token_type": "Bearer", "expires_in": 32400})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"name":"UNAUTHORIZED","message":"Authorization failed due to insufficient permissions."}`))
			return
		}
		query := r.URL.Query()
		switch r.URL.Path {
		case "/v1/reporting/transactions":
			start, err := time.Parse(stamp, query.Get("start_date"))
			if err != nil {
				t.Errorf("the start date reads in the report's form: %s", query.Get("start_date"))
			}
			end, err := time.Parse(stamp, query.Get("end_date"))
			if err != nil {
				t.Errorf("the end date reads in the report's form: %s", query.Get("end_date"))
			}
			if end.Sub(start) > 31*24*time.Hour {
				t.Errorf("a window is at most thirty-one days: %s", r.URL.RawQuery)
			}
			if query.Get("fields") != "transaction_info,payer_info" {
				t.Errorf("the report asks for the transaction and payer fields: %s", r.URL.RawQuery)
			}
			kept := []map[string]any{}
			for _, row := range transactions {
				when, _ := time.Parse(stamp, row["transaction_info"].(map[string]any)["transaction_initiation_date"].(string))
				if !when.Before(start) && !when.After(end) {
					kept = append(kept, row)
				}
			}
			size, _ := strconv.Atoi(query.Get("page_size"))
			page, _ := strconv.Atoi(query.Get("page"))
			pages := int(math.Ceil(float64(len(kept)) / float64(size)))
			from := (page - 1) * size
			if from > len(kept) {
				from = len(kept)
			}
			to := from + size
			if to > len(kept) {
				to = len(kept)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"transaction_details": kept[from:to], "page": page, "total_items": len(kept), "total_pages": pages})
		case "/v2/invoicing/invoices":
			if query.Get("total_required") != "true" {
				t.Errorf("the invoice list asks for its total: %s", r.URL.RawQuery)
			}
			size, _ := strconv.Atoi(query.Get("page_size"))
			page, _ := strconv.Atoi(query.Get("page"))
			pages := int(math.Ceil(float64(len(invoices)) / float64(size)))
			from := (page - 1) * size
			if from > len(invoices) {
				from = len(invoices)
			}
			to := from + size
			if to > len(invoices) {
				to = len(invoices)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"items": invoices[from:to], "total_items": len(invoices), "total_pages": pages})
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"name":"RESOURCE_NOT_FOUND","message":"The specified resource does not exist."}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(secret string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "app", "client_secret": secret})
	return string(body)
}

// open points a source at the fake with the clock stopped after every
// fixture, so the newest window holds them all.
func open(t *testing.T, server *httptest.Server) *Source {
	t.Helper()
	source := New(packed("hush"), server.URL)
	source.now = func() time.Time { return time.Date(2023, 11, 20, 0, 0, 0, 0, time.UTC) }
	return source
}

func TestObjectsSignInOnceAndRefuseBadCredentials(t *testing.T) {
	server, calls := fakePayPal(t)
	source := open(t, server)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "transactions" || objects[1].Label != "Invoices" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/v1/oauth2/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if live := New(packed("hush"), ""); live.baseURL != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.baseURL)
	}
	_, err = New(packed("wrong"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_client") {
		t.Fatalf("a refused client secret must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "client secret") {
		t.Fatalf("one bare value names the two credentials, got %v", err)
	}
	_, err = source.Describe("payouts")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	stale := open(t, server)
	if err := stale.signIn(); err != nil {
		t.Fatal(err)
	}
	stale.token = "session-0"
	_, err = stale.Page("invoices", nil, 1)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("a refused access token reads as not_connected, got %v", err)
	}
	denied := []byte(`{"name":"NOT_AUTHORIZED","message":"Authorization failed due to insufficient permissions.","details":[{"issue":"PERMISSION_DENIED","description":"You do not have permission to access or perform operations on this resource."}]}`)
	err = source.refusal("transactions", http.StatusForbidden, denied)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Transaction Search") || !strings.Contains(failure.Message, "You do not have permission") {
		t.Fatalf("a 403 names the feature the app lacks and the issue, got %v", err)
	}
	source.lastRetry = "30"
	if err := source.refusal("invoices", http.StatusTooManyRequests, nil); err.(*Failure).Code != "rate_limited" || !strings.Contains(err.Error(), "30 seconds") {
		t.Fatalf("a 429 reads the Retry-After header, got %v", err)
	}
}

func TestDescribeCountsInvoicesAndFlattensMoney(t *testing.T) {
	server, _ := fakePayPal(t)
	description, err := open(t, server).Describe("invoices")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Invoices" || description.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["total"].Guess != "number" || byName["invoice_date"].Guess != "date" || byName["customer_email"].Guess != "email" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["total"].Samples; len(got) != 3 || got[0] != "1200.00" || got[1] != "50.00" || got[2] != "10.00" {
		t.Fatalf("totals read as the decimals PayPal writes: %v", got)
	}
	if got := byName["paid"].Samples; len(got) != 2 || got[0] != "1000.00" || got[1] != "50.00" || byName["paid"].Filled != 2 {
		t.Fatalf("paid amounts: %v", got)
	}
	if got := byName["status"].Samples; len(got) != 3 || got[0] != "sent" || got[1] != "paid" || got[2] != "draft" {
		t.Fatalf("statuses read lowered: %v", got)
	}
	if byName["currency"].Samples[1] != "EUR" || byName["due_date"].Samples[0] != "2023-12-01" || byName["payment_term"].Samples[0] != "NET_30" || byName["created"].Samples[0] != "2023-11-01T08:00:00Z" {
		t.Fatalf("currencies, terms and times: %v %v %v %v", byName["currency"].Samples, byName["due_date"].Samples, byName["payment_term"].Samples, byName["created"].Samples)
	}
	if byName["customer_name"].Samples[0] != "Ann Lee" || byName["customer_name"].Samples[1] != "Bob Ray" || byName["customer_business"].Filled != 1 || byName["customer_email_domain"].Samples[0] != "northwind.example" {
		t.Fatalf("recipients: %v %v", byName["customer_name"].Samples, byName["customer_email_domain"].Samples)
	}
	transactions, err := open(t, server).Describe("transactions")
	if err != nil {
		t.Fatal(err)
	}
	if transactions.Rows != 3 || transactions.Counted || transactions.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("the report counts nothing, so rows are the rows read: %+v", transactions)
	}
}

func TestPageWalksInvoicePagesAndTransactionWindows(t *testing.T) {
	server, calls := fakePayPal(t)
	source := open(t, server)
	first, err := source.Page("invoices", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first invoice page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["invoice_number"] != "0001" || row["reference"] != "PO 9" || row["total"] != "1200.00" || row["due"] != "200.00" || row["paid"] != "1000.00" || row["currency"] != "USD" || row["recipient_view_url"] != "https://www.paypal.com/invoice/p/INV2-1" {
		t.Fatalf("invoice row: %v", row)
	}
	second, err := source.Page("invoices", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "INV2-3" || second.Rows[0]["status"] != "draft" {
		t.Fatalf("second invoice page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "/v2/invoicing/invoices?page=2&page_size=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not walk the page number: %v", *calls)
	}
	report, err := source.Page("transactions", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(report.Rows) != 2 || report.Total != 2 || report.Counted || report.Next == nil || report.Next.Token != "2023-11-20T00:00:00Z|2" || report.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("first transaction page: %+v next %+v", report, report.Next)
	}
	item := report.Rows[0]
	if item["id"] != "T1" || item["status"] != "completed" || item["amount"] != "120.00" || item["fee"] != "-3.78" || item["currency"] != "USD" || item["invoice_id"] != "INV-1" || item["reference_type"] != "odr" {
		t.Fatalf("transaction row: %v", item)
	}
	if item["payer_id"] != "P1" || item["payer_name"] != "Ann Lee" || item["payer_email_domain"] != "northwind.example" || item["payer_country"] != "US" || item["created"] != "2023-11-01T10:00:00Z" || item["modified"] != "2023-11-15T11:00:00Z" {
		t.Fatalf("payer columns: %v", item)
	}
	if report.Rows[1]["status"] != "pending" || report.Rows[1]["payer_name"] != "Bob Ray" {
		t.Fatalf("a payer without a full name reads as given name and surname: %v", report.Rows[1])
	}
	rest, err := source.Page("transactions", report.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(rest.Rows) != 1 || rest.Offset != 2 || rest.Total != 3 || rest.Rows[0]["status"] != "reversed" || rest.Rows[0]["amount"] != "-50.00" || rest.Next == nil || rest.Next.Token != "2023-10-20T00:00:00Z|1" {
		t.Fatalf("the last page of a window steps to the window before it: %+v next %+v", rest, rest.Next)
	}
	end, err := source.Page("transactions", rest.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(end.Rows) != 0 || end.Next != nil || end.Offset != 3 || end.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("empty windows walk to the floor and end: %+v", end)
	}
	if _, err := source.Page("transactions", &Cursor{Token: "garbage"}, 2); err == nil || err.(*Failure).Code != "source" {
		t.Fatalf("a cursor that does not parse reads as a source failure, got %v", err)
	}
}

func TestDeltaReadsTheNewestPageAndComparesTheMark(t *testing.T) {
	server, calls := fakePayPal(t)
	source := open(t, server)
	delta, err := source.Delta("transactions", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("transactions", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00Z"})
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
	delta, err = source.Delta("invoices", &Cursor{Offset: 3, Hash: "2023-11-16T08:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("an invoice delta at the mark is unchanged: %+v", delta)
	}
	reports, lists := 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "/v1/reporting/transactions?") && strings.Contains(call, "page=1&page_size=500&start_date=2023-10-20T00%3A00%3A00%2B0000") {
			reports++
		}
		if strings.Contains(call, "/v2/invoicing/invoices?page=1&page_size=100&total_required=true") {
			lists++
		}
	}
	if reports != 2 || lists != 2 {
		t.Fatalf("each delta is one read of the newest window or page: %d %d in %v", reports, lists, *calls)
	}
}
