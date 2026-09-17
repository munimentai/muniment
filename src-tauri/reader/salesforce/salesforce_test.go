package salesforce

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeSalesforce signs a connected app in, describes objects, answers SOQL
// from fixtures in batches through a locator, and refuses a wrong secret.
func fakeSalesforce(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	accounts := []map[string]any{
		{"Id": "001A", "Name": "Northwind Traders", "Website": "https://www.northwind.example/about", "Industry": "Technology", "Type": "Customer", "Phone": "+1 555 010 0000",
			"BillingCity": "Seattle", "BillingState": "WA", "BillingCountry": "US", "NumberOfEmployees": 120.0, "AnnualRevenue": 4500000.0, "OwnerId": "005X",
			"CreatedDate": "2023-01-01T00:00:00.000+0000", "LastModifiedDate": "2023-11-15T11:00:00.000+0000", "Renewal_Risk__c": 0.2},
		{"Id": "001B", "Name": "Contoso", "Website": "contoso.example", "OwnerId": "005X", "CreatedDate": "2023-02-01T00:00:00.000+0000", "LastModifiedDate": "2023-11-14T11:00:00.000+0000"},
		{"Id": "001C", "Name": "Fabrikam", "OwnerId": "005Y", "CreatedDate": "2023-03-01T00:00:00.000+0000", "LastModifiedDate": "2023-11-13T11:00:00.000+0000", "Renewal_Risk__c": 0.9},
	}
	opportunities := []map[string]any{
		{"Id": "006A", "Name": "Northwind renewal", "Amount": 96000.0, "StageName": "Negotiation/Review", "IsClosed": false, "IsWon": false, "Probability": 80.0, "CloseDate": "2023-12-16", "AccountId": "001A", "OwnerId": "005X",
			"CreatedDate": "2023-10-01T00:00:00.000+0000", "LastModifiedDate": "2023-11-16T08:00:00.000+0000"},
		{"Id": "006B", "Name": "Contoso pilot", "StageName": "Signed", "IsClosed": true, "IsWon": true, "AccountId": "001B", "LastModifiedDate": "2023-11-10T08:00:00.000+0000"},
		{"Id": "006C", "Name": "Fabrikam", "StageName": "Dead", "IsClosed": true, "IsWon": false, "AccountId": "001C", "LastModifiedDate": "2023-11-09T08:00:00.000+0000"},
	}
	cases := []map[string]any{
		{"Id": "500A", "CaseNumber": "00001001", "Subject": "Invoice mismatch", "Status": "On Hold", "IsClosed": false, "Priority": "High", "Origin": "Email", "AccountId": "001A", "ContactId": "003A",
			"CreatedDate": "2023-11-16T08:30:00.000+0000", "LastModifiedDate": "2023-11-16T09:00:00.000+0000"},
	}
	// Two hundred more accounts, older than the three, so a full batch pages.
	for index := 0; index < 200; index++ {
		accounts = append(accounts, map[string]any{"Id": "001X" + strconv.Itoa(index), "Name": "Auto " + strconv.Itoa(index), "OwnerId": "005X",
			"CreatedDate": "2022-01-01T00:00:00.000+0000", "LastModifiedDate": "2022-06-01T00:00:00.000+0000"})
	}
	data := map[string][]map[string]any{"Account": accounts, "Opportunity": opportunities, "Case": cases, "Contact": {}, "Lead": {}}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery+" "+r.Header.Get("Sforce-Query-Options"))
		if r.URL.Path == "/services/oauth2/token" {
			_ = r.ParseForm()
			if r.Form.Get("grant_type") != "client_credentials" || r.Form.Get("client_id") != "key" || r.Form.Get("client_secret") != "good" {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_client","error_description":"invalid client credentials"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "instance_url": "http://" + r.Host, "token_type": "Bearer"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`[{"message":"Session expired or invalid","errorCode":"INVALID_SESSION_ID"}]`))
			return
		}
		parts := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
		if len(parts) == 6 && parts[3] == "sobjects" && parts[5] == "describe" {
			switch parts[4] {
			case "Account":
				_ = json.NewEncoder(w).Encode(map[string]any{"fields": []any{
					map[string]any{"name": "Name", "type": "string", "custom": false},
					map[string]any{"name": "Renewal_Risk__c", "type": "percent", "custom": true},
					map[string]any{"name": "HQ__c", "type": "address", "custom": true},
				}})
			case "Case":
				w.WriteHeader(http.StatusForbidden)
				_, _ = w.Write([]byte(`[{"message":"insufficient access","errorCode":"INSUFFICIENT_ACCESS"}]`))
			default:
				_ = json.NewEncoder(w).Encode(map[string]any{"fields": []any{}})
			}
			return
		}
		batch := 2000
		if option := r.Header.Get("Sforce-Query-Options"); strings.HasPrefix(option, "batchSize=") {
			batch, _ = strconv.Atoi(strings.TrimPrefix(option, "batchSize="))
		}
		var rows []map[string]any
		start := 0
		switch {
		case len(parts) == 4 && parts[3] == "query":
			soql := r.URL.Query().Get("q")
			from := strings.Fields(soql[strings.Index(soql, " FROM ")+6:])[0]
			rows = data[from]
			if from == "Case" && strings.Contains(soql, "Renewal_Risk__c") {
				t.Errorf("a describe refusal must leave the custom fields out: %s", soql)
			}
			if strings.Contains(soql, "WHERE LastModifiedDate > ") {
				mark := strings.Fields(soql[strings.Index(soql, "WHERE LastModifiedDate > ")+25:])[0]
				kept := []map[string]any{}
				for _, row := range rows {
					if row["LastModifiedDate"].(string) > mark {
						kept = append(kept, row)
					}
				}
				rows = kept
			}
			if strings.Contains(soql, "ORDER BY LastModifiedDate DESC") {
				sorted := append([]map[string]any(nil), rows...)
				for i := range sorted {
					for j := i + 1; j < len(sorted); j++ {
						if sorted[j]["LastModifiedDate"].(string) > sorted[i]["LastModifiedDate"].(string) {
							sorted[i], sorted[j] = sorted[j], sorted[i]
						}
					}
				}
				rows = sorted
				if strings.HasSuffix(soql, "LIMIT 1") && len(rows) > 1 {
					rows = rows[:1]
				}
			}
		case len(parts) == 5 && parts[3] == "query":
			locator := strings.Split(parts[4], "-")
			rows = data[locator[0]]
			start, _ = strconv.Atoi(locator[1])
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`[{"message":"no route","errorCode":"NOT_FOUND"}]`))
			return
		}
		end := start + batch
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{"totalSize": len(rows), "done": end >= len(rows), "records": rows[start:end]}
		if end < len(rows) {
			from := rows[0]["Id"].(string)[:3]
			name := map[string]string{"001": "Account", "006": "Opportunity", "500": "Case"}[from]
			answer["nextRecordsUrl"] = "/services/data/v60.0/query/" + name + "-" + strconv.Itoa(end)
		}
		_ = json.NewEncoder(w).Encode(answer)
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func credentials(server *httptest.Server, secret string) string {
	packed, _ := json.Marshal(map[string]string{"instance_url": server.URL + "/", "client_id": "key", "client_secret": secret})
	return string(packed)
}

func TestObjectsSignInOnceAndListFive(t *testing.T) {
	server, calls := fakeSalesforce(t)
	source := New(credentials(server, "good"))
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 5 || objects[0].Name != "accounts" || objects[4].Label != "Cases" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/services/oauth2/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the session is exchanged once per process: %d in %v", signIns, *calls)
	}
	_, err = New(credentials(server, "bad")).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_client") {
		t.Fatalf("a refused app must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New("just-a-token").Objects()
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "My Domain URL") {
		t.Fatalf("one bare value names the three credentials, got %v", err)
	}
	_, err = source.Describe("quotes")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestDescribeCountsAndFlattensAccountsWithCustomFields(t *testing.T) {
	server, calls := fakeSalesforce(t)
	description, err := New(credentials(server, "good")).Describe("accounts")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 203 || !description.Counted || description.Label != "Accounts" || description.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["domain"].Guess != "domain" || byName["parent"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["domain"].Samples; len(got) != 2 || got[0] != "northwind.example" || got[1] != "contoso.example" {
		t.Fatalf("domains read from websites with and without a scheme: %v", got)
	}
	if byName["created"].Samples[0] != "2023-01-01T00:00:00Z" || byName["employees"].Samples[0] != "120" {
		t.Fatalf("times and numbers: %v %v", byName["created"].Samples, byName["employees"].Samples)
	}
	risk, ok := byName["renewal_risk"]
	if !ok || risk.Guess != "number" || risk.Filled != 2 || risk.Samples[0] != "0.2" {
		t.Fatalf("the custom field is a column without its suffix: %+v", risk)
	}
	if _, address := byName["hq"]; address {
		t.Fatal("a compound address field must not become a column")
	}
	soql := ""
	for _, call := range *calls {
		if strings.Contains(call, "/query?") {
			soql = call
		}
	}
	for _, need := range []string{"Renewal_Risk__c", "FROM+Account", "ORDER+BY+Id", "batchSize=200"} {
		if !strings.Contains(soql, need) {
			t.Fatalf("the query lacks %s: %s", need, soql)
		}
	}
}

func TestPageWalksTheLocatorAndFoldsStagesAndStatuses(t *testing.T) {
	server, calls := fakeSalesforce(t)
	source := New(credentials(server, "good"))
	first, err := source.Page("accounts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 200 || first.Offset != 0 || first.Total != 203 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if !strings.HasSuffix(first.Next.Token, "/query/Account-200") || first.Next.Offset != 200 || first.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	if first.Rows[2]["id"] != "001C" || first.Rows[2]["renewal_risk"] != "0.9" || first.Rows[0]["domain"] != "northwind.example" {
		t.Fatalf("account rows: %v", first.Rows[2])
	}
	second, err := source.Page("accounts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 3 || second.Offset != 200 || second.Total != 203 || second.Next != nil || second.Rows[0]["id"] != "001X197" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "/query/Account-200?") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not walk the locator: %v", *calls)
	}
	batches := 0
	for _, call := range *calls {
		if strings.Contains(call, "/query") && strings.HasSuffix(call, "batchSize=200") {
			batches++
		}
	}
	if batches < 2 {
		t.Fatalf("a small limit reads as the smallest batch Salesforce honors: %v", *calls)
	}

	opportunities, err := source.Page("opportunities", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := opportunities.Rows[0]
	if row["name"] != "Northwind renewal" || row["amount"] != "96000" || row["stage"] != "negotiation" || row["stage_name"] != "Negotiation/Review" || row["won"] != "no" || row["close_date"] != "2023-12-16" {
		t.Fatalf("opportunity row: %v", row)
	}
	if opportunities.Rows[1]["stage"] != "won" || opportunities.Rows[2]["stage"] != "lost" {
		t.Fatalf("a custom stage folds by closed and won: %v %v", opportunities.Rows[1], opportunities.Rows[2])
	}

	cases, err := source.Page("cases", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := cases.Rows[0]
	if item["number"] != "00001001" || item["status"] != "pending" || item["status_name"] != "On Hold" || item["priority"] != "high" || item["account"] != "001A" || item["created"] != "2023-11-16T08:30:00Z" {
		t.Fatalf("case row: %v", item)
	}
}

func TestDeltaQueriesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeSalesforce(t)
	source := New(credentials(server, "good"))
	delta, err := source.Delta("accounts", &Cursor{Offset: 203, Hash: "2023-11-15T11:00:00.000Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("accounts", &Cursor{Offset: 203, Hash: "2023-11-14T00:00:00.000Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("opportunities", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00.000Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	queries := 0
	for _, call := range *calls {
		if strings.Contains(call, "LastModifiedDate+DESC") {
			queries++
		}
	}
	if queries != 3 {
		t.Fatalf("each delta is one query: %d in %v", queries, *calls)
	}
}

func TestRefusalsReadTheErrorCode(t *testing.T) {
	limit := []byte(`[{"message":"TotalRequests Limit exceeded.","errorCode":"REQUEST_LIMIT_EXCEEDED"}]`)
	failure, _ := refusal(http.StatusForbidden, limit).(*Failure)
	if failure == nil || failure.Code != "rate_limited" || !strings.Contains(failure.Message, "tomorrow") {
		t.Fatalf("a spent daily budget waits a day: %v", failure)
	}
	failure, _ = refusal(http.StatusForbidden, []byte(`[{"message":"insufficient access","errorCode":"INSUFFICIENT_ACCESS"}]`)).(*Failure)
	if failure == nil || failure.Code != "not_connected" || !strings.Contains(failure.Message, "run-as user") {
		t.Fatalf("an access refusal names the run-as user: %v", failure)
	}
	if refusal(http.StatusOK, nil) != nil {
		t.Fatal("a 200 is not a refusal")
	}
	if formatTime("2023-11-14T22:13:20.000+0000") != "2023-11-14T22:13:20Z" || formatTime("2023-11-14") != "2023-11-14T00:00:00Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if websiteDomain(record{"Website": "HTTP://WWW.Example.COM/x"}) != "example.com" || websiteDomain(record{"Website": ""}) != "" {
		t.Fatal("website domains did not read")
	}
	if guessType("currency") != "number" || guessType("datetime") != "date-time" || guessType("picklist") != "string" {
		t.Fatal("field types did not guess")
	}
}
