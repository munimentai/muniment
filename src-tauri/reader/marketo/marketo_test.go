package marketo

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// fakeMarketo grants a token for one client, issues paging tokens that
// carry their start time, answers leads and companies changed after that
// time in batches, answers programs by offset and updated range, and
// refuses wrong credentials the way Marketo does: a 401 at the identity
// endpoint and a 200 with success false everywhere else.
func fakeMarketo(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	leads := []map[string]any{
		{"id": 1, "firstName": "Ann", "lastName": "Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "company": "Northwind Traders", "title": "CFO", "leadStatus": "Qualified", "leadSource": "Web", "industry": "Retail", "leadScore": 42,
			"website": "https://www.northwind.example/about", "city": "Seattle", "state": "WA", "postalCode": "98101", "country": "US", "unsubscribed": false, "createdAt": "2023-01-01T08:00:00Z", "updatedAt": "2023-11-15T11:00:00Z"},
		{"id": 2, "firstName": "Bob", "lastName": "Ray", "email": "bob@contoso.example", "company": "Contoso", "leadScore": 0, "unsubscribed": true, "createdAt": "2023-02-01T08:00:00Z", "updatedAt": "2023-11-14T11:00:00Z"},
		{"id": 3, "firstName": "Cy", "lastName": "Fox", "createdAt": "2023-03-01T08:00:00Z", "updatedAt": "2023-11-13T11:00:00Z"},
	}
	companies := []map[string]any{
		{"id": 11, "company": "Northwind Traders", "externalCompanyId": "NW-1", "website": "northwind.example", "industry": "Retail", "annualRevenue": 1250000.5, "numberOfEmployees": 250, "mainPhone": "+1 555 010 0000", "billingCity": "Seattle", "billingState": "WA", "billingCountry": "US", "createdAt": "2023-01-01T08:00:00Z", "updatedAt": "2023-11-12T11:00:00Z"},
	}
	programs := []map[string]any{
		{"id": 1001, "name": "Fall launch", "description": "The fall launch", "type": "Default", "channel": "Email", "status": "", "workspace": "Default", "folder": map[string]any{"type": "Folder", "value": 55, "folderName": "Launches"}, "url": "https://app.example/#PG1001A1",
			"tags": []any{map[string]any{"tagType": "Region", "tagValue": "West"}, map[string]any{"tagType": "Owner", "tagValue": "Ann"}}, "costs": []any{map[string]any{"startDate": "2023-09-01", "cost": 1000.0}, map[string]any{"startDate": "2023-10-01", "cost": 250.5}},
			"createdAt": "2023-09-01T08:00:00Z+0000", "updatedAt": "2023-11-16T08:00:00Z+0000"},
		{"id": 1002, "name": "Webinar", "type": "Event", "channel": "Webinar", "status": "Active", "workspace": "Default", "folder": map[string]any{"type": "Folder", "value": 56, "folderName": "Events"}, "startDate": "2023-11-20T10:00:00Z+0000", "endDate": "2023-11-20T11:00:00Z+0000",
			"createdAt": "2023-10-01T08:00:00Z+0000", "updatedAt": "2023-11-11T08:00:00Z+0000"},
	}
	paged := map[string][]map[string]any{"/rest/v1/leads.json": leads, "/rest/v1/companies.json": companies}
	after := func(rows []map[string]any, since string) []map[string]any {
		floor, ok := parseTime(since)
		if !ok {
			t.Errorf("the since time does not parse: %s", since)
		}
		kept := []map[string]any{}
		for _, row := range rows {
			if updated, _ := parseTime(row["updatedAt"].(string)); updated.After(floor) {
				kept = append(kept, row)
			}
		}
		return kept
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		query := r.URL.Query()
		if r.URL.Path == "/identity/oauth/token" {
			if query.Get("grant_type") != "client_credentials" || query.Get("client_id") != "app" || query.Get("client_secret") != "hush" {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"invalid_client","error_description":"Bad client credentials"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "token_type": "bearer", "expires_in": 3599, "scope": "api@acme.example"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "errors": []any{map[string]any{"code": "601", "message": "Access token invalid"}}})
			return
		}
		switch r.URL.Path {
		case "/rest/v1/activities/pagingtoken.json":
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "nextPageToken": "tok|" + query.Get("sinceDatetime") + "|0"})
		case "/rest/v1/leads.json", "/rest/v1/companies.json":
			parts := strings.Split(query.Get("nextPageToken"), "|")
			if len(parts) != 3 || parts[0] != "tok" || query.Get("fields") == "" {
				_ = json.NewEncoder(w).Encode(map[string]any{"success": false, "errors": []any{map[string]any{"code": "1001", "message": "Bad paging token or no fields"}}})
				return
			}
			rows := after(paged[r.URL.Path], parts[1])
			from, _ := strconv.Atoi(parts[2])
			size, _ := strconv.Atoi(query.Get("batchSize"))
			if from > len(rows) {
				from = len(rows)
			}
			end := from + size
			if end > len(rows) {
				end = len(rows)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "result": rows[from:end], "nextPageToken": "tok|" + parts[1] + "|" + strconv.Itoa(end), "moreResult": end < len(rows)})
		case "/rest/asset/v1/programs.json":
			rows := programs
			if since := query.Get("earliestUpdatedAt"); since != "" {
				if query.Get("latestUpdatedAt") != "2023-11-20T00:00:00Z" {
					t.Errorf("an updated range names its end as now: %s", r.URL.RawQuery)
				}
				rows = after(rows, since)
			}
			from, _ := strconv.Atoi(query.Get("offset"))
			size, _ := strconv.Atoi(query.Get("maxReturn"))
			if from > len(rows) {
				from = len(rows)
			}
			end := from + size
			if end > len(rows) {
				end = len(rows)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "result": rows[from:end]})
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"errors":[{"code":"404","message":"no such path"}]}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(secret string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "app", "client_secret": secret, "munchkin_id": "123-ABC-456"})
	return string(body)
}

func open(t *testing.T, server *httptest.Server) *Source {
	t.Helper()
	source := New(packed("hush"), server.URL)
	source.now = func() time.Time { return time.Date(2023, 11, 20, 0, 0, 0, 0, time.UTC) }
	return source
}

func TestObjectsSignInOnceAndRefuseBadCredentials(t *testing.T) {
	server, calls := fakeMarketo(t)
	source := open(t, server)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "leads" || objects[2].Label != "Programs" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/identity/oauth/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if live := New(packed("hush"), ""); live.baseURL != "https://123-abc-456.mktorest.com" {
		t.Fatalf("an empty base URL is the Munchkin host: %s", live.baseURL)
	}
	_, err = New(packed("wrong"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_client") {
		t.Fatalf("a refused client secret must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Munchkin id") {
		t.Fatalf("one bare value names the three credentials, got %v", err)
	}
	_, err = source.Describe("opportunities")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	expired := []byte(`{"success":false,"errors":[{"code":"602","message":"Access token expired"}]}`)
	if err := refusal(http.StatusOK, expired); err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "expired") {
		t.Fatalf("a 200 with error 602 must read as not_connected, got %v", err)
	}
	denied := []byte(`{"success":false,"errors":[{"code":"603","message":"Access denied"}]}`)
	if err := refusal(http.StatusOK, denied); err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "role") {
		t.Fatalf("a 200 with error 603 must read as not_connected naming the role, got %v", err)
	}
	limited := []byte(`{"success":false,"errors":[{"code":"606","message":"Max rate limit exceeded"}]}`)
	if err := refusal(http.StatusOK, limited); err.(*Failure).Code != "rate_limited" {
		t.Fatalf("a 200 with error 606 must read as rate_limited, got %v", err)
	}
	if err := refusal(http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a read must read as not_connected, got %v", err)
	}
	if err := refusal(http.StatusOK, []byte(`{"success":true,"result":[]}`)); err != nil {
		t.Fatalf("a successful answer passes, got %v", err)
	}
	stale := open(t, server)
	if err := stale.signIn(); err != nil {
		t.Fatal(err)
	}
	stale.token = "session-0"
	_, err = stale.Page("leads", nil, 1)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Access token invalid") {
		t.Fatalf("a token Marketo refuses in a 200 reads as not_connected, got %v", err)
	}
}

func TestDescribeSamplesTheFirstPageAndFlattensProgramsWithCosts(t *testing.T) {
	server, _ := fakeMarketo(t)
	description, err := open(t, server).Describe("programs")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || description.Counted || description.Label != "Programs" || description.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["folder_id"].Guess != "id" || byName["cost"].Guess != "number" || byName["start_date"].Guess != "date" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["cost"].Samples; len(got) != 1 || got[0] != "1250.50" || byName["cost"].Filled != 1 {
		t.Fatalf("the cost is the sum of the cost lines as a decimal: %v", got)
	}
	if got := byName["tags"].Samples; len(got) != 1 || got[0] != "Owner:Ann,Region:West" {
		t.Fatalf("tags read as sorted type:value pairs: %v", got)
	}
	if got := byName["type"].Samples; len(got) != 2 || got[0] != "default" || got[1] != "event" {
		t.Fatalf("types read lowered: %v", got)
	}
	if byName["folder_id"].Samples[0] != "55" || byName["folder_name"].Samples[1] != "Events" || byName["start_date"].Samples[0] != "2023-11-20" || byName["status"].Filled != 1 {
		t.Fatalf("folders, dates and fills: %v %v %v %d", byName["folder_id"].Samples, byName["folder_name"].Samples, byName["start_date"].Samples, byName["status"].Filled)
	}
	if byName["modified"].Samples[0] != "2023-11-16T08:00:00Z" {
		t.Fatalf("the asset stamp reads as RFC 3339 in UTC: %v", byName["modified"].Samples)
	}
}

func TestPageWalksThePagingTokenAndFlattensLeadsAndCompanies(t *testing.T) {
	server, calls := fakeMarketo(t)
	source := open(t, server)
	first, err := source.Page("leads", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if first.Next.Token != "tok|"+floor+"|2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["id"] != "1" || row["email_domain"] != "northwind.example" || row["domain"] != "northwind.example" || row["job_title"] != "CFO" || row["lead_status"] != "Qualified" || row["score"] != "42" || row["state"] != "WA" || row["unsubscribed"] != "no" {
		t.Fatalf("lead row: %v", row)
	}
	if first.Rows[1]["unsubscribed"] != "yes" || first.Rows[1]["domain"] != "" || first.Rows[1]["created"] != "2023-02-01T08:00:00Z" {
		t.Fatalf("second lead: %v", first.Rows[1])
	}
	second, err := source.Page("leads", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	tokens, walked := 0, false
	for _, call := range *calls {
		if strings.Contains(call, "/rest/v1/activities/pagingtoken.json") {
			tokens++
		}
		if strings.Contains(call, "/rest/v1/leads.json?batchSize=2&fields=id%2CfirstName") && strings.Contains(call, "nextPageToken=tok%7C"+strings.ReplaceAll(floor, ":", "%3A")+"%7C2") {
			walked = true
		}
	}
	if tokens != 1 || !walked {
		t.Fatalf("the first page asks for one paging token and the second walks the next page token with the fields: %d %v in %v", tokens, walked, *calls)
	}
	companies, err := source.Page("companies", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := companies.Rows[0]
	if item["id"] != "11" || item["name"] != "Northwind Traders" || item["external_id"] != "NW-1" || item["domain"] != "northwind.example" || item["annual_revenue"] != "1250000.50" || item["employees"] != "250" || item["state"] != "WA" {
		t.Fatalf("company row: %v", item)
	}
	if companies.Next != nil {
		t.Fatalf("the last batch ends the walk: %+v", companies.Next)
	}
	programs, err := source.Page("programs", nil, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(programs.Rows) != 1 || programs.Next == nil || programs.Next.Token != "1" {
		t.Fatalf("a full program page names the next offset: %+v", programs)
	}
	last, err := source.Page("programs", programs.Next, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(last.Rows) != 1 || last.Rows[0]["id"] != "1002" || last.Next == nil {
		t.Fatalf("the second program page walks the offset: %+v", last)
	}
	if end, err := source.Page("programs", last.Next, 1); err != nil || len(end.Rows) != 0 || end.Next != nil {
		t.Fatalf("an empty program page ends the walk: %+v %v", end, err)
	}
}

func TestDeltaQueriesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeMarketo(t)
	source := open(t, server)
	delta, err := source.Delta("leads", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("leads", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("programs", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	delta, err = source.Delta("programs", &Cursor{Offset: 2, Hash: "2023-11-16T08:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("a program delta at the mark is unchanged: %+v", delta)
	}
	marked, ranged := 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "pagingtoken.json?sinceDatetime=2023-11-14T00%3A00%3A00Z") {
			marked++
		}
		if strings.Contains(call, "programs.json?earliestUpdatedAt=2023-11-16T08%3A00%3A00Z&latestUpdatedAt=2023-11-20T00%3A00%3A00Z&maxReturn=1&offset=0") {
			ranged++
		}
	}
	if marked != 1 || ranged != 1 {
		t.Fatalf("the mark travels in the paging token and the program range: %d %d in %v", marked, ranged, *calls)
	}
}
