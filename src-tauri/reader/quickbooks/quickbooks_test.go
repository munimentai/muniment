package quickbooks

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

var (
	fromClause  = regexp.MustCompile(`FROM (\w+)`)
	startClause = regexp.MustCompile(`STARTPOSITION (\d+)`)
	maxClause   = regexp.MustCompile(`MAXRESULTS (\d+)`)
	whereClause = regexp.MustCompile(`WHERE MetaData.LastUpdatedTime > '([^']+)'`)
)

// fakeQuickBooks exchanges a refresh token, answers the query endpoint from
// fixtures by start position, counts, filters by change time, and refuses
// a wrong token.
func fakeQuickBooks(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	meta := func(created, updated string) map[string]any {
		return map[string]any{"CreateTime": created, "LastUpdatedTime": updated}
	}
	customers := []map[string]any{
		{"Id": "1", "DisplayName": "Northwind Traders", "CompanyName": "Northwind Traders", "GivenName": "Ann", "FamilyName": "Lee", "PrimaryEmailAddr": map[string]any{"Address": "Ann@Northwind.example"},
			"PrimaryPhone": map[string]any{"FreeFormNumber": "+1 555 010 0000"}, "WebAddr": map[string]any{"URI": "https://www.northwind.example/about"}, "Balance": 200.5, "CurrencyRef": map[string]any{"value": "USD"},
			"Active": true, "BillAddr": map[string]any{"City": "Seattle", "CountrySubDivisionCode": "WA", "PostalCode": "98101", "Country": "US"}, "MetaData": meta("2023-01-01T00:00:00-08:00", "2023-11-15T03:00:00-08:00")},
		{"Id": "2", "DisplayName": "Contoso", "Balance": 0.0, "Active": true, "ParentRef": map[string]any{"value": "1"}, "MetaData": meta("2023-02-01T00:00:00-08:00", "2023-11-14T03:00:00-08:00")},
		{"Id": "3", "DisplayName": "Fabrikam", "Balance": 12.0, "Active": false, "MetaData": meta("2023-03-01T00:00:00-08:00", "2023-11-13T03:00:00-08:00")},
	}
	invoices := []map[string]any{
		{"Id": "101", "DocNumber": "1001", "CustomerRef": map[string]any{"value": "1", "name": "Northwind Traders"}, "BillEmail": map[string]any{"Address": "ap@northwind.example"}, "TotalAmt": 1200.0, "Balance": 200.0,
			"CurrencyRef": map[string]any{"value": "USD"}, "TxnDate": "2023-11-01", "DueDate": "2023-12-01", "EmailStatus": "EmailSent", "CustomerMemo": map[string]any{"value": "Thank you"}, "MetaData": meta("2023-11-01T00:00:00-08:00", "2023-11-16T00:00:00-08:00")},
		{"Id": "102", "DocNumber": "1002", "CustomerRef": map[string]any{"value": "2"}, "TotalAmt": 50.0, "Balance": 0.0, "CurrencyRef": map[string]any{"value": "EUR"}, "TxnDate": "2023-11-02", "DueDate": "2023-11-20", "MetaData": meta("2023-11-02T00:00:00-08:00", "2023-11-10T00:00:00-08:00")},
	}
	payments := []map[string]any{
		{"Id": "201", "CustomerRef": map[string]any{"value": "1", "name": "Northwind Traders"}, "TotalAmt": 1000.0, "UnappliedAmt": 0.0, "CurrencyRef": map[string]any{"value": "USD"}, "TxnDate": "2023-11-05", "PaymentRefNum": "CHK 9",
			"PaymentMethodRef": map[string]any{"value": "2", "name": "Check"}, "Line": []any{map[string]any{"Amount": 1000.0, "LinkedTxn": []any{map[string]any{"TxnId": "101", "TxnType": "Invoice"}}}}, "MetaData": meta("2023-11-05T00:00:00-08:00", "2023-11-05T00:00:00-08:00")},
	}
	data := map[string][]map[string]any{"Customer": customers, "Invoice": invoices, "Payment": payments}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/oauth2/v1/tokens/bearer" {
			_ = r.ParseForm()
			user, pass, _ := r.BasicAuth()
			if user != "app" || pass != "hush" || r.Form.Get("grant_type") != "refresh_token" || r.Form.Get("refresh_token") != "good" {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_grant","error_description":"Incorrect or invalid refresh token"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "refresh_token": "next", "token_type": "bearer"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"Fault":{"Error":[{"Message":"message=AuthenticationFailed; errorCode=003200","Detail":"Token expired","code":"3200"}],"type":"AUTHENTICATION"}}`))
			return
		}
		if r.URL.Path != "/v3/company/realm1/query" {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"Fault":{"Error":[{"Message":"no such company","code":"6000"}]}}`))
			return
		}
		if r.URL.Query().Get("minorversion") != MinorVersion {
			t.Errorf("every query names the minor version: %s", r.URL.RawQuery)
		}
		soql := r.URL.Query().Get("query")
		entity := fromClause.FindStringSubmatch(soql)[1]
		rows := data[entity]
		if strings.Contains(soql, "COUNT(*)") {
			_ = json.NewEncoder(w).Encode(map[string]any{"QueryResponse": map[string]any{"totalCount": len(rows)}})
			return
		}
		if match := whereClause.FindStringSubmatch(soql); match != nil {
			kept := []map[string]any{}
			for _, row := range rows {
				if formatTime(row["MetaData"].(map[string]any)["LastUpdatedTime"].(string)) > formatTime(match[1]) {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if strings.Contains(soql, "ORDERBY MetaData.LastUpdatedTime DESC") {
			sorted := append([]map[string]any(nil), rows...)
			for i := range sorted {
				for j := i + 1; j < len(sorted); j++ {
					if formatTime(sorted[j]["MetaData"].(map[string]any)["LastUpdatedTime"].(string)) > formatTime(sorted[i]["MetaData"].(map[string]any)["LastUpdatedTime"].(string)) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		start := 1
		if match := startClause.FindStringSubmatch(soql); match != nil {
			start, _ = strconv.Atoi(match[1])
		}
		limit := 100
		if match := maxClause.FindStringSubmatch(soql); match != nil {
			limit, _ = strconv.Atoi(match[1])
		}
		from := start - 1
		if from > len(rows) {
			from = len(rows)
		}
		end := from + limit
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{"startPosition": start, "maxResults": end - from}
		if end > from {
			answer[entity] = rows[from:end]
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"QueryResponse": answer, "time": "2023-11-16T00:00:00-08:00"})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(refresh, realm string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "app", "client_secret": "hush", "refresh_token": refresh, "realm_id": realm})
	return string(body)
}

func TestObjectsSignInOnceAndProveTheRealm(t *testing.T) {
	server, calls := fakeQuickBooks(t)
	source := New(packed("good", "realm1"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "customers" || objects[2].Label != "Payments" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/oauth2/v1/tokens/bearer") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if live := New(packed("good", "realm1"), ""); live.baseURL != DefaultBaseURL || live.tokenURL != DefaultTokenURL {
		t.Fatalf("an empty base URL is the live API and the live token endpoint: %s %s", live.baseURL, live.tokenURL)
	}
	_, err = New(packed("bad", "realm1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_grant") {
		t.Fatalf("a refused refresh token must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New(packed("good", "realm2"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "source" || !strings.Contains(failure.Message, "no such company") {
		t.Fatalf("a wrong realm reads the fault message, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "realm id") {
		t.Fatalf("one bare value names the four credentials, got %v", err)
	}
	_, err = source.Describe("bills")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a query must read as not_connected, got %v", err)
	}
}

func TestDescribeCountsAndFlattensInvoicesWithMoney(t *testing.T) {
	server, _ := fakeQuickBooks(t)
	description, err := New(packed("good", "realm1"), server.URL).Describe("invoices")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || !description.Counted || description.Label != "Invoices" || description.Hash != "2023-11-16T08:00:00+00:00" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["customer_id"].Guess != "id" || byName["total"].Guess != "number" || byName["date"].Guess != "date" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["total"].Samples; len(got) != 2 || got[0] != "1200.00" || got[1] != "50.00" {
		t.Fatalf("totals read as decimals in the major unit: %v", got)
	}
	if got := byName["amount_paid"].Samples; len(got) != 2 || got[0] != "1000.00" || got[1] != "50.00" {
		t.Fatalf("amount paid is the total less the balance: %v", got)
	}
	if got := byName["status"].Samples; len(got) != 2 || got[0] != "overdue" || got[1] != "paid" {
		t.Fatalf("statuses fold from balance and due date: %v", got)
	}
	if byName["currency"].Samples[1] != "EUR" || byName["created"].Samples[0] != "2023-11-01T08:00:00Z" || byName["memo"].Filled != 1 {
		t.Fatalf("currencies, times and fills: %v %v %d", byName["currency"].Samples, byName["created"].Samples, byName["memo"].Filled)
	}
}

func TestPageWalksTheStartPositionAndFlattensCustomersAndPayments(t *testing.T) {
	server, calls := fakeQuickBooks(t)
	source := New(packed("good", "realm1"), server.URL)
	first, err := source.Page("customers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "3" || first.Next.Offset != 2 || first.Hash != "2023-11-15T11:00:00+00:00" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["name"] != "Northwind Traders" || row["email_domain"] != "northwind.example" || row["domain"] != "northwind.example" || row["balance"] != "200.50" || row["currency"] != "USD" || row["active"] != "yes" || row["state"] != "WA" {
		t.Fatalf("customer row: %v", row)
	}
	if first.Rows[1]["parent_id"] != "1" || first.Rows[1]["balance"] != "0.00" {
		t.Fatalf("the parent reads as its own column: %v", first.Rows[1])
	}
	second, err := source.Page("customers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" || second.Rows[0]["active"] != "no" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00+00:00" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "STARTPOSITION+3+MAXRESULTS+2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not walk the start position: %v", *calls)
	}
	payments, err := source.Page("payments", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := payments.Rows[0]
	if item["invoice_id"] != "101" || item["customer_id"] != "1" || item["total"] != "1000.00" || item["currency"] != "USD" || item["method"] != "Check" || item["reference"] != "CHK 9" || item["date"] != "2023-11-05" {
		t.Fatalf("payment row: %v", item)
	}
}

func TestDeltaQueriesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeQuickBooks(t)
	source := New(packed("good", "realm1"), server.URL)
	delta, err := source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00+00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("customers", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00+00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00+00:00" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("invoices", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00+00:00" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	queries := 0
	for _, call := range *calls {
		if strings.Contains(call, "LastUpdatedTime+DESC+MAXRESULTS+1") {
			queries++
		}
	}
	if queries != 3 {
		t.Fatalf("each delta is one query: %d in %v", queries, *calls)
	}
}
