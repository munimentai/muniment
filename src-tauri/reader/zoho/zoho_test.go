package zoho

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeZoho answers the token endpoint, the four module lists and their
// field settings from fixtures, pages by page number, honors
// If-Modified-Since, and refuses a wrong refresh token.
func fakeZoho(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	leads := []map[string]any{
		{"id": "1001", "Full_Name": "Ann Lee", "First_Name": "Ann", "Last_Name": "Lee", "Company": "Northwind Traders", "Email": "Ann@Northwind.example", "Phone": "+1 555 010 0000", "Designation": "CFO",
			"Lead_Status": "Contacted", "Lead_Source": "Web", "Owner": map[string]any{"id": "7", "name": "Mikey"}, "Created_Time": "2023-11-14T22:13:20+05:30", "Modified_Time": "2023-11-15T15:30:00+05:30", "Tier": "gold"},
		{"id": "1002", "Full_Name": "Bo Fabrik", "Email": "bo@fabrikam.example", "Owner": map[string]any{"id": "7", "name": "Mikey"}, "Created_Time": "2023-11-13T09:00:00Z", "Modified_Time": "2023-11-13T09:00:00Z"},
		{"id": "1003", "Full_Name": "Cy Contoso", "Created_Time": "2023-11-12T09:00:00Z", "Modified_Time": "2023-11-12T09:00:00Z"},
	}
	contacts := []map[string]any{
		{"id": "2001", "Full_Name": "Ann Lee", "Email": "ann@northwind.example", "Title": "CFO", "Account_Name": map[string]any{"id": "3001", "name": "Northwind Traders"}, "Owner": map[string]any{"id": "7", "name": "Mikey"}, "Created_Time": "2023-01-01T00:00:00Z", "Modified_Time": "2023-11-15T11:00:00Z"},
	}
	accounts := []map[string]any{
		{"id": "3001", "Account_Name": "Northwind Traders", "Website": "https://www.northwind.example/about", "Industry": "Retail", "Employees": 120.0, "Annual_Revenue": 5000000.0, "Parent_Account": map[string]any{"id": "3000", "name": "Northwind Holdings"}, "Created_Time": "2023-01-01T00:00:00Z", "Modified_Time": "2023-11-15T12:00:00Z"},
	}
	deals := []map[string]any{
		{"id": "4001", "Deal_Name": "Northwind renewal", "Amount": 96000.0, "Currency": "USD", "Stage": "Negotiation/Review", "Probability": 80.0, "Closing_Date": "2023-12-16", "Account_Name": map[string]any{"id": "3001", "name": "Northwind Traders"}, "Contact_Name": map[string]any{"id": "2001", "name": "Ann Lee"}, "Created_Time": "2023-10-01T00:00:00Z", "Modified_Time": "2023-11-16T08:00:00Z"},
		{"id": "4002", "Deal_Name": "Contoso pilot", "Amount": 1200.0, "Stage": "Closed Won", "Created_Time": "2023-09-01T00:00:00Z", "Modified_Time": "2023-11-10T08:00:00Z"},
		{"id": "4003", "Deal_Name": "Fabrikam", "Stage": "Closed-Lost to Competition", "Created_Time": "2023-09-02T00:00:00Z", "Modified_Time": "2023-11-09T08:00:00Z"},
		{"id": "4004", "Deal_Name": "Early", "Stage": "Qualification", "Created_Time": "2023-09-03T00:00:00Z", "Modified_Time": "2023-11-08T08:00:00Z"},
		{"id": "4005", "Deal_Name": "Middle", "Stage": "Proposal/Price Quote", "Created_Time": "2023-09-04T00:00:00Z", "Modified_Time": "2023-11-07T08:00:00Z"},
	}
	data := map[string][]map[string]any{"/crm/v2/Leads": leads, "/crm/v2/Contacts": contacts, "/crm/v2/Accounts": accounts, "/crm/v2/Deals": deals}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/oauth/v2/token" {
			_ = r.ParseForm()
			if r.Form.Get("grant_type") != "refresh_token" || r.Form.Get("client_id") != "cid" || r.Form.Get("client_secret") != "csecret" {
				_ = json.NewEncoder(w).Encode(map[string]any{"error": "invalid_client"})
				return
			}
			switch r.Form.Get("refresh_token") {
			case "rt-good":
				_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "at-good", "api_domain": "https://www.zohoapis.example", "expires_in": 3600})
			default:
				_ = json.NewEncoder(w).Encode(map[string]any{"error": "invalid_code"})
			}
			return
		}
		switch r.Header.Get("Authorization") {
		case "Zoho-oauthtoken at-good":
		case "Zoho-oauthtoken at-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"code":"TOO_MANY_REQUESTS","message":"You have exceeded the API limit"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"code":"INVALID_TOKEN","details":{},"message":"invalid oauth token","status":"error"}`))
			return
		}
		if r.URL.Path == "/crm/v2/settings/fields" {
			switch r.URL.Query().Get("module") {
			case "Leads":
				_ = json.NewEncoder(w).Encode(map[string]any{"fields": []any{
					map[string]any{"api_name": "Full_Name", "field_label": "Full Name", "data_type": "text", "custom_field": false},
					map[string]any{"api_name": "Tier", "field_label": "Customer Tier", "data_type": "picklist", "custom_field": true},
				}})
			default:
				_ = json.NewEncoder(w).Encode(map[string]any{"fields": []any{}})
			}
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusBadRequest)
			_, _ = w.Write([]byte(`{"code":"INVALID_MODULE","message":"the module name given seems to be invalid","status":"error"}`))
			return
		}
		if since := r.Header.Get("If-Modified-Since"); since != "" {
			changed := []map[string]any{}
			for _, row := range rows {
				if formatTime(row["Modified_Time"].(string)) > since {
					changed = append(changed, row)
				}
			}
			rows = changed
		}
		if r.URL.Query().Get("sort_by") == "Modified_Time" && r.URL.Query().Get("sort_order") == "desc" {
			sorted := append([]map[string]any(nil), rows...)
			for i := 0; i < len(sorted); i++ {
				for j := i + 1; j < len(sorted); j++ {
					if formatTime(sorted[j]["Modified_Time"].(string)) > formatTime(sorted[i]["Modified_Time"].(string)) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		if len(rows) == 0 {
			w.WriteHeader(http.StatusNotModified)
			return
		}
		perPage, _ := strconv.Atoi(r.URL.Query().Get("per_page"))
		if perPage <= 0 {
			perPage = 200
		}
		page, _ := strconv.Atoi(r.URL.Query().Get("page"))
		if page <= 0 {
			page = 1
		}
		start := (page - 1) * perPage
		if start > len(rows) {
			start = len(rows)
		}
		end := start + perPage
		if end > len(rows) {
			end = len(rows)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": rows[start:end], "info": map[string]any{
			"per_page": perPage, "count": end - start, "page": page, "more_records": end < len(rows),
		}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(refreshToken string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "cid", "client_secret": "csecret", "refresh_token": refreshToken, "accounts_domain": "accounts.zoho.eu"})
	return string(body)
}

func TestObjectsSignInOnceAndListFour(t *testing.T) {
	server, calls := fakeZoho(t)
	source := New(packed("rt-good"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "leads" || objects[3].Label != "Deals" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "POST /oauth/v2/token") {
		t.Fatalf("objects is the one sign-in call: %v", *calls)
	}
	if source.baseURL != server.URL {
		t.Fatalf("a pinned base URL ignores the api_domain the token names: %s", source.baseURL)
	}
	if live := New(packed("x"), ""); live.accounts != "https://accounts.zoho.eu" || live.baseURL != "https://www.zohoapis.eu" {
		t.Fatalf("the accounts domain names the data center: %s %s", live.accounts, live.baseURL)
	}
	if live := New(`{"client_id":"a","client_secret":"b","refresh_token":"c"}`, ""); live.accounts != DefaultAccountsURL || live.baseURL != DefaultBaseURL {
		t.Fatalf("no accounts domain is the US data center: %s %s", live.accounts, live.baseURL)
	}
	_, err = New(packed("rt-bad"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_code") {
		t.Fatalf("a refused refresh token must read as not_connected, got %v", err)
	}
	_, err = New("bare", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "client id, client secret, refresh token") {
		t.Fatalf("one bare value names the credentials, got %v", err)
	}
	_, err = source.Describe("quotes")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestRefusalsReadTheStatus(t *testing.T) {
	server, _ := fakeZoho(t)
	source := New(packed("rt-good"), server.URL)
	source.token = "at-stale"
	_, err := source.Page("leads", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the access token") {
		t.Fatalf("a 401 must read as not_connected, got %v", err)
	}
	source.token = "at-slow"
	_, err = source.Page("leads", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
	if err := refusal(http.StatusOK, []byte(`{"code":"NO_PERMISSION","message":"permission denied"}`)); err == nil || err.(*Failure).Code != "not_connected" {
		t.Fatalf("a permission code reads as not_connected whatever the status, got %v", err)
	}
}

func TestDescribeFlattensLeadsWithCustomFields(t *testing.T) {
	server, calls := fakeZoho(t)
	description, err := New(packed("rt-good"), server.URL).Describe("leads")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Leads" || description.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["owner_id"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email_domain"].Samples; len(got) != 2 || got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["owner"].Samples[0] != "Mikey" || byName["owner_id"].Samples[0] != "7" || byName["created"].Samples[0] != "2023-11-14T16:43:20Z" {
		t.Fatalf("references and times: %+v %+v %+v", byName["owner"], byName["owner_id"], byName["created"])
	}
	tier, ok := byName["customer_tier"]
	if !ok || tier.Guess != "string" || tier.Filled != 1 || tier.Samples[0] != "gold" {
		t.Fatalf("the custom field is a column named as the org named it: %+v", tier)
	}
	if _, raw := byName["Tier"]; raw {
		t.Fatal("the raw API name must not be a column")
	}
	fieldCalls := 0
	for _, call := range *calls {
		if strings.Contains(call, "/crm/v2/settings/fields?module=Leads") {
			fieldCalls++
		}
	}
	if fieldCalls != 1 {
		t.Fatalf("the field settings read once: %v", *calls)
	}
}

func TestPageWalksPagesAndFoldsDeals(t *testing.T) {
	server, calls := fakeZoho(t)
	source := New(packed("rt-good"), server.URL)
	first, err := source.Page("leads", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("leads", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "1003" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/crm/v2/Leads?") && strings.Contains(call, "page=2") && strings.Contains(call, "per_page=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not ask for page 2: %v", *calls)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "POST /oauth/v2/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the source signs in once per process: %d", signIns)
	}

	deals, err := source.Page("deals", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := deals.Rows[0]
	if row["name"] != "Northwind renewal" || row["amount"] != "96000" || row["stage_name"] != "Negotiation/Review" || row["stage"] != "negotiation" {
		t.Fatalf("deal row: %v", row)
	}
	if row["account_id"] != "3001" || row["account_name"] != "Northwind Traders" || row["contact_id"] != "2001" || row["close_date"] != "2023-12-16" {
		t.Fatalf("deal references and dates: %v", row)
	}
	stages := []string{}
	for _, entry := range deals.Rows {
		stages = append(stages, entry["stage"])
	}
	if strings.Join(stages, ",") != "negotiation,won,lost,qualification,proposal" {
		t.Fatalf("stages fold by the default names: %v", stages)
	}

	accounts, err := source.Page("accounts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if accounts.Rows[0]["domain"] != "northwind.example" || accounts.Rows[0]["parent_id"] != "3000" || accounts.Rows[0]["employees"] != "120" {
		t.Fatalf("account row: %v", accounts.Rows[0])
	}
}

func TestDeltaAsksSinceTheMark(t *testing.T) {
	server, calls := fakeZoho(t)
	source := New(packed("rt-good"), server.URL)
	delta, err := source.Delta("leads", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("leads", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("deals", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	listCalls := 0
	for _, call := range *calls {
		if strings.Contains(call, "/crm/v2/Leads?") && strings.Contains(call, "per_page=1") {
			listCalls++
		}
	}
	if listCalls != 2 {
		t.Fatalf("each delta is one list call of one record: %d in %v", listCalls, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20+05:30") != "2023-11-14T16:43:20Z" || formatTime("2023-12-16") != "2023-12-16T00:00:00Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if columnName("Customer Tier") != "customer_tier" || columnName(" Setup fee (USD) ") != "setup_fee_usd" {
		t.Fatal("column names did not fold")
	}
	if customValue(map[string]any{"id": "9", "name": "Ann"}) != "Ann" || customValue([]any{"b", "a"}) != "a,b" || customValue(12.5) != "12.5" {
		t.Fatal("custom values did not read")
	}
	if newestModified(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" || newestModified([]record{{"Modified_Time": "2022-01-01T00:00:00Z"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
	if guessType("currency") != "number" || guessType("datetime") != "date-time" || guessType("picklist") != "string" {
		t.Fatal("field types did not guess")
	}
	if apiHost("https://accounts.zoho.in") != "https://www.zohoapis.in" || apiHost("https://accounts.example.com") != DefaultBaseURL {
		t.Fatal("the API host did not derive")
	}
}
