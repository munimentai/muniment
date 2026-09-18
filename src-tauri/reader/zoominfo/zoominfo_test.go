package zoominfo

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// fakeZoomInfo signs a JWT for the right password, answers the two
// searches from fixtures, pages by page number with counts, sorts and
// filters by lastUpdatedDate, and refuses a wrong JWT.
func fakeZoomInfo(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	contacts := []map[string]any{
		{"id": 1.0, "firstName": "Ann", "lastName": "Lee", "jobTitle": "CFO", "hasEmail": true, "hasPhone": true, "contactAccuracyScore": 95.0, "company": map[string]any{"id": 10.0, "name": "Northwind Traders"}, "validDate": "11/14/2023 10:13:20 PM", "lastUpdatedDate": "2023-11-15T10:00:00.000Z"},
		{"id": 2.0, "firstName": "Bo", "lastName": "Fabrik", "hasEmail": false, "company": map[string]any{"id": 11.0, "name": "Fabrikam"}, "lastUpdatedDate": "2023-11-13T09:00:00.000Z"},
		{"id": 3.0, "firstName": "Cy", "lastUpdatedDate": "2023-11-12T09:00:00.000Z"},
	}
	companies := []map[string]any{
		{"id": 10.0, "name": "Northwind Traders", "website": "https://www.northwind.example/about", "employeeCount": 120.0, "revenue": 25000.0, "primaryIndustry": "Retail", "lastUpdatedDate": "2023-11-15T11:00:00.000Z"},
	}
	data := map[string][]map[string]any{"/search/contact": contacts, "/search/company": companies}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		encoded, _ := json.Marshal(body)
		calls = append(calls, r.Method+" "+r.URL.Path+" "+string(encoded))
		if r.URL.Path == "/authenticate" {
			switch body["password"] {
			case "pass-good":
				_ = json.NewEncoder(w).Encode(map[string]any{"jwt": "jwt-good"})
			default:
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"status":401,"error":"Unauthorized","message":"Bad credentials"}`))
			}
			return
		}
		switch r.Header.Get("Authorization") {
		case "Bearer jwt-good":
		case "Bearer jwt-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"status":429,"message":"Too many requests"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"status":401,"error":"Unauthorized","message":"Invalid JWT"}`))
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok || r.Method != http.MethodPost {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"status":404,"message":"Not Found"}`))
			return
		}
		if after, _ := body["lastUpdatedDateAfter"].(string); after != "" {
			kept := []map[string]any{}
			for _, row := range rows {
				if row["lastUpdatedDate"].(string)[:10] >= after {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if body["sortBy"] == "lastUpdatedDate" {
			ascending := body["sortOrder"] == "asc"
			sorted := append([]map[string]any(nil), rows...)
			for i := 0; i < len(sorted); i++ {
				for j := i + 1; j < len(sorted); j++ {
					later := sorted[j]["lastUpdatedDate"].(string) > sorted[i]["lastUpdatedDate"].(string)
					if later != ascending {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		perPage := int(body["rpp"].(float64))
		if perPage <= 0 {
			perPage = 25
		}
		page := int(body["page"].(float64))
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
		_ = json.NewEncoder(w).Encode(map[string]any{"maxResults": len(rows), "totalResults": len(rows), "currentPage": page, "data": rows[start:end]})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

const goodSecret = `{"username":"api@northwind.example","password":"pass-good"}`

func TestObjectsSignInAndListTwo(t *testing.T) {
	server, calls := fakeZoomInfo(t)
	source := New(goodSecret, server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "contacts" || objects[1].Label != "Companies" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "POST /authenticate ") || !strings.Contains((*calls)[0], `"username":"api@northwind.example"`) {
		t.Fatalf("the account was proved with one sign-in: %v", *calls)
	}
	if live := New(goodSecret, ""); live.base != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.base)
	}
	_, err = New(`{"username":"api@northwind.example","password":"pass-bad"}`, server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Bad credentials") {
		t.Fatalf("a refused password must read as not_connected, got %v", err)
	}
	_, err = New("bare-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Connect ZoomInfo with") {
		t.Fatalf("a bare token is not a username and password, got %v", err)
	}
	slow := New(goodSecret, server.URL)
	slow.token = "jwt-slow"
	_, err = slow.Describe("contacts")
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	stale := New(goodSecret, server.URL)
	stale.token = "jwt-stale"
	_, err = stale.Describe("contacts")
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the JWT") {
		t.Fatalf("a refused JWT must read as not_connected, got %v", err)
	}
	_, err = source.Describe("intents")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"status":403,"message":"Your plan does not include company search"}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "plan does not include") {
		t.Fatalf("a 403 names the error, got %v", err)
	}
}

func TestDescribeCountsAndFlattensContacts(t *testing.T) {
	server, calls := fakeZoomInfo(t)
	source := New(goodSecret, server.URL)
	description, err := source.Describe("contacts")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Contacts" || description.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["company_id"].Guess != "id" || byName["modified"].Guess != "date-time" || byName["has_email"].Guess != "boolean" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["company_id"].Samples[0] != "11" || byName["company_name"].Samples[1] != "Northwind Traders" || byName["name"].Samples[0] != "Cy" || byName["name"].Samples[2] != "Ann Lee" {
		t.Fatalf("references and names: %+v %+v", byName["company_id"], byName["name"])
	}
	if byName["has_email"].Samples[0] != "no" || byName["has_email"].Samples[1] != "yes" || byName["valid_at"].Samples[0] != "2023-11-14T22:13:20Z" || byName["accuracy_score"].Samples[0] != "95" {
		t.Fatalf("flags, times and scores: %v %v %v", byName["has_email"].Samples, byName["valid_at"].Samples, byName["accuracy_score"].Samples)
	}
	if byName["company_id"].Filled != 2 {
		t.Fatalf("two contacts carry a company: %+v", byName["company_id"])
	}
	if _, err := source.Describe("companies"); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "POST /authenticate ") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the JWT is obtained once per process: %v", *calls)
	}
}

func TestPageWalksNumberedPages(t *testing.T) {
	server, calls := fakeZoomInfo(t)
	source := New(goodSecret, server.URL)
	first, err := source.Page("contacts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-13T09:00:00Z" || first.Rows[0]["id"] != "3" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("contacts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "1" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/search/contact") && strings.Contains(call, `"page":2`) && strings.Contains(call, `"rpp":2`) && strings.Contains(call, `"sortOrder":"asc"`) {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not ask for page 2 oldest first: %v", *calls)
	}

	companies, err := source.Page("companies", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := companies.Rows[0]
	if row["name"] != "Northwind Traders" || row["website"] != "https://www.northwind.example/about" || row["domain"] != "northwind.example" || row["employees"] != "120" || row["industry"] != "Retail" || row["modified"] != "2023-11-15T11:00:00Z" {
		t.Fatalf("company row: %v", row)
	}
	if companies.Next != nil || companies.Total != 1 {
		t.Fatalf("one company is one page: %+v", companies)
	}
}

func TestDeltaFiltersAfterTheMark(t *testing.T) {
	server, calls := fakeZoomInfo(t)
	source := New(goodSecret, server.URL)
	delta, err := source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("contacts", &Cursor{Offset: 3, Hash: "2024-01-01T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("a filter that matches nothing is unchanged: %+v", delta)
	}
	delta, err = source.Delta("companies", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.Contains(call, "/search/contact") && strings.Contains(call, `"lastUpdatedDateAfter":"`) && strings.Contains(call, `"rpp":1`) && strings.Contains(call, `"sortOrder":"desc"`) {
			filtered++
		}
	}
	if filtered != 3 {
		t.Fatalf("each marked delta is one filtered search: %d in %v", filtered, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("11/14/2023 10:13:20 PM") != "2023-11-14T22:13:20Z" || formatTime("2023-11-14T22:13:20.000Z") != "2023-11-14T22:13:20Z" || formatTime("2023-11-14") != "2023-11-14T00:00:00Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" || newestUpdated([]record{{"lastUpdatedDate": "1/1/2022 12:00:00 AM"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
	if websiteDomain("website")(record{"website": "northwind.example"}) != "northwind.example" || websiteDomain("website")(record{}) != "" {
		t.Fatal("a bare host reads as its domain")
	}
	if zoominfoMessage([]byte(`{"success":false,"data":{"error":"Invalid page"}}`)) != "Invalid page" || zoominfoMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a failure reads its message")
	}
}
