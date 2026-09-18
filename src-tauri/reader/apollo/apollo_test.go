package apollo

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// fakeApollo answers the two searches from fixtures, pages by page number
// with counts, sorts by updated_at when asked, and refuses a wrong key.
func fakeApollo(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	contacts := []map[string]any{
		{"id": "c1", "first_name": "Ann", "last_name": "Lee", "name": "Ann Lee", "email": "Ann@Northwind.example", "email_status": "verified", "title": "CFO", "organization_name": "Northwind Traders",
			"account_id": "a10", "organization_id": "o10", "owner_id": "u7", "contact_stage_id": "s2", "linkedin_url": "https://linkedin.example/ann", "city": "Seattle", "state": "WA", "country": "US",
			"phone_numbers": []any{map[string]any{"raw_number": "+1 (555) 010-0000", "sanitized_number": "+15550100000", "type": "work"}}, "label_ids": []any{"vip", "finance"},
			"last_activity_date": "2023-11-14T22:13:20.000Z", "created_at": "2023-11-14T22:13:20.000Z", "updated_at": "2023-11-15T10:00:00.123Z"},
		{"id": "c2", "first_name": "Bo", "last_name": "Fabrik", "email": "bo@fabrikam.example", "phone_numbers": []any{}, "sanitized_phone": "+15550100001", "account_id": nil, "created_at": "2023-11-13T09:00:00.000Z", "updated_at": "2023-11-13T09:00:00.000Z"},
		{"id": "c3", "email": "cy@contoso.example", "created_at": "2023-11-12T09:00:00.000Z", "updated_at": "2023-11-12T09:00:00.000Z"},
	}
	accounts := []map[string]any{
		{"id": "a10", "name": "Northwind Traders", "domain": "northwind.example", "website_url": "https://northwind.example", "phone": "+1 555 010 0000", "sanitized_phone": "+15550100000", "industry": "retail",
			"organization_id": "o10", "owner_id": "u7", "account_stage_id": "s5", "city": "Seattle", "country": "US", "label_ids": []any{}, "created_at": "2023-01-01T00:00:00.000Z", "updated_at": "2023-11-15T11:00:00.000Z"},
	}
	data := map[string]struct {
		key  string
		rows []map[string]any
	}{
		"/api/v1/contacts/search": {"contacts", contacts},
		"/api/v1/accounts/search": {"accounts", accounts},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		encoded, _ := json.Marshal(body)
		calls = append(calls, r.Method+" "+r.URL.Path+" "+string(encoded))
		switch r.Header.Get("x-api-key") {
		case "key-good":
		case "key-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"error":"Rate limit exceeded"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":"Invalid access credentials"}`))
			return
		}
		listed, ok := data[r.URL.Path]
		if !ok || r.Method != http.MethodPost {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":"Not Found"}`))
			return
		}
		rows := append([]map[string]any(nil), listed.rows...)
		if sort, _ := body["sort_by_field"].(string); sort != "" {
			ascending, _ := body["sort_ascending"].(bool)
			for i := 0; i < len(rows); i++ {
				for j := i + 1; j < len(rows); j++ {
					later := rows[j]["updated_at"].(string) > rows[i]["updated_at"].(string)
					if later != ascending {
						rows[i], rows[j] = rows[j], rows[i]
					}
				}
			}
		}
		perPage := int(body["per_page"].(float64))
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
		_ = json.NewEncoder(w).Encode(map[string]any{listed.key: rows[start:end], "pagination": map[string]any{
			"page": page, "per_page": perPage, "total_entries": len(rows), "total_pages": (len(rows) + perPage - 1) / perPage,
		}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheKeyAndListTwo(t *testing.T) {
	server, calls := fakeApollo(t)
	source := New("key-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "contacts" || objects[1].Label != "Accounts" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "POST /api/v1/contacts/search") || !strings.Contains((*calls)[0], `"per_page":1`) {
		t.Fatalf("the key was not proved with one small search: %v", *calls)
	}
	if live := New("x", ""); live.base != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.base)
	}
	_, err = New("key-bad", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the API key") {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = New("key-slow", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("sequences")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"error":"This endpoint is not on your plan"}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "not on your plan") {
		t.Fatalf("a 403 names the error, got %v", err)
	}
}

func TestDescribeCountsAndFlattensContacts(t *testing.T) {
	server, _ := fakeApollo(t)
	description, err := New("key-good", server.URL).Describe("contacts")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Contacts" || description.Hash != "2023-11-15T10:00:00.123Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["account_id"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["email_domain"].Samples[0] != "contoso.example" || byName["account_id"].Samples[0] != "a10" || byName["owner_id"].Samples[0] != "u7" || byName["stage_id"].Samples[0] != "s2" {
		t.Fatalf("domains and references: %+v %+v", byName["email_domain"], byName["account_id"])
	}
	if byName["name"].Samples[0] != "cy@contoso.example" || byName["name"].Samples[1] != "Bo Fabrik" || byName["name"].Samples[2] != "Ann Lee" {
		t.Fatalf("names fall back to first and last, then the email: %v", byName["name"].Samples)
	}
	if byName["phone"].Samples[0] != "+15550100001" || byName["phone"].Samples[1] != "+15550100000" {
		t.Fatalf("phones read the sanitized number: %v", byName["phone"].Samples)
	}
	if byName["created"].Samples[0] != "2023-11-12T09:00:00Z" || byName["labels"].Samples[0] != "finance,vip" || byName["company_name"].Filled != 1 {
		t.Fatalf("times, labels and companies: %v %v %d", byName["created"].Samples, byName["labels"].Samples, byName["company_name"].Filled)
	}
}

func TestPageWalksNumberedPages(t *testing.T) {
	server, calls := fakeApollo(t)
	source := New("key-good", server.URL)
	first, err := source.Page("contacts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-13T09:00:00.000Z" || first.Rows[0]["id"] != "c3" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("contacts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "c1" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00.123Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/api/v1/contacts/search") && strings.Contains(call, `"page":2`) && strings.Contains(call, `"per_page":2`) && strings.Contains(call, `"sort_ascending":true`) {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not ask for page 2 oldest first: %v", *calls)
	}

	accounts, err := source.Page("accounts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := accounts.Rows[0]
	if row["name"] != "Northwind Traders" || row["domain"] != "northwind.example" || row["phone"] != "+15550100000" || row["owner_id"] != "u7" || row["stage_id"] != "s5" || row["labels"] != "" || row["modified"] != "2023-11-15T11:00:00Z" {
		t.Fatalf("account row: %v", row)
	}
	if accounts.Next != nil || accounts.Total != 1 {
		t.Fatalf("one account is one page: %+v", accounts)
	}
}

func TestDeltaReadsTheNewestChange(t *testing.T) {
	server, calls := fakeApollo(t)
	source := New("key-good", server.URL)
	delta, err := source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00.123Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("contacts", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00.000Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00.123Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("accounts", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	newestFirst := 0
	for _, call := range *calls {
		if strings.Contains(call, `"per_page":1`) && strings.Contains(call, `"sort_ascending":false`) && strings.Contains(call, `_updated_at`) {
			newestFirst++
		}
	}
	if newestFirst != 3 {
		t.Fatalf("each delta is one search for the newest change: %d in %v", newestFirst, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.000Z") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00.000Z") != "2023-01-01T00:00:00.000Z" || newestUpdated([]record{{"updated_at": "2022-01-01T00:00:00Z"}}, "2023-01-01T00:00:00.000Z") != "2023-01-01T00:00:00.000Z" {
		t.Fatal("the mark never moves backwards")
	}
	if apolloMessage([]byte(`{"error_code":"invalid","error_message":"The key is not valid"}`)) != "The key is not valid" || apolloMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a failure reads its message")
	}
	if contactPhone(record{"phone_numbers": []any{map[string]any{"raw_number": "555"}}}) != "555" || contactPhone(record{}) != "" {
		t.Fatal("a raw number reads when nothing is sanitized")
	}
}
