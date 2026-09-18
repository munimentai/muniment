package salesloft

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeSalesloft answers the three lists and the custom field definitions
// from fixtures, pages by page number with counts, filters by updated_at,
// and refuses a wrong key.
func fakeSalesloft(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	people := []map[string]any{
		{"id": 1.0, "first_name": "Ann", "last_name": "Lee", "display_name": "Ann Lee", "email_address": "Ann@Northwind.example", "phone": "+1 555 010 0000", "title": "CFO", "person_company_name": "Northwind Traders",
			"account": map[string]any{"id": 10.0}, "owner": map[string]any{"id": 7.0}, "person_stage": map[string]any{"id": 2.0}, "do_not_contact": false, "tags": []any{"vip", "finance"},
			"last_contacted_at": "2023-11-14T22:13:20.000000Z", "created_at": "2023-11-14T22:13:20.000000Z", "updated_at": "2023-11-15T10:00:00.123456Z",
			"custom_fields": map[string]any{"Customer Tier": "gold", "Regions": []any{"west", "east"}}},
		{"id": 2.0, "first_name": "Bo", "last_name": "Fabrik", "email_address": "bo@fabrikam.example", "account": nil, "do_not_contact": true, "created_at": "2023-11-13T09:00:00.000000Z", "updated_at": "2023-11-13T09:00:00.000000Z", "custom_fields": map[string]any{}},
		{"id": 3.0, "email_address": "cy@contoso.example", "created_at": "2023-11-12T09:00:00.000000Z", "updated_at": "2023-11-12T09:00:00.000000Z"},
	}
	accounts := []map[string]any{
		{"id": 10.0, "name": "Northwind Traders", "domain": "northwind.example", "website": "https://northwind.example", "industry": "Retail", "company_type": "Private", "size": "51-200", "city": "Seattle", "country": "US",
			"owner": map[string]any{"id": 7.0}, "company_stage": map[string]any{"id": 5.0}, "tags": []any{}, "archived_at": nil, "created_at": "2023-01-01T00:00:00.000000Z", "updated_at": "2023-11-15T11:00:00.000000Z",
			"custom_fields": map[string]any{"Segment": "enterprise"}},
	}
	cadences := []map[string]any{
		{"id": 20.0, "name": "Renewal", "cadence_type": "team", "current_state": "active", "shared": true, "team_cadence": true, "owner": map[string]any{"id": 7.0}, "tags": []any{"q4"}, "created_at": "2023-01-01T00:00:00.000000Z", "updated_at": "2023-11-15T12:00:00.000000Z"},
	}
	data := map[string][]map[string]any{"/v2/people.json": people, "/v2/accounts.json": accounts, "/v2/cadences.json": cadences}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		switch r.Header.Get("Authorization") {
		case "Bearer key-good":
		case "Bearer key-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"status":429,"error":"Rate limit exceeded"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"status":401,"error":"Unauthorized"}`))
			return
		}
		if r.URL.Path == "/v2/custom_fields.json" {
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{
				map[string]any{"id": 1.0, "name": "Customer Tier", "field_type": "person"},
				map[string]any{"id": 2.0, "name": "Regions", "field_type": "person"},
				map[string]any{"id": 3.0, "name": "Segment", "field_type": "company"},
				map[string]any{"id": 4.0, "name": "Forecast", "field_type": "opportunity"},
			}, "metadata": map[string]any{"paging": map[string]any{"per_page": 100, "current_page": 1, "next_page": nil, "prev_page": nil}}})
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"status":404,"error":"Not Found"}`))
			return
		}
		query := r.URL.Query()
		if since := query.Get("updated_at[gt]"); since != "" {
			kept := []map[string]any{}
			for _, row := range rows {
				if row["updated_at"].(string) > since {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if query.Get("sort_by") == "updated_at" && query.Get("sort_direction") == "DESC" {
			sorted := append([]map[string]any(nil), rows...)
			for i := 0; i < len(sorted); i++ {
				for j := i + 1; j < len(sorted); j++ {
					if sorted[j]["updated_at"].(string) > sorted[i]["updated_at"].(string) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		perPage, _ := strconv.Atoi(query.Get("per_page"))
		if perPage <= 0 {
			perPage = 25
		}
		page, _ := strconv.Atoi(query.Get("page"))
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
		paging := map[string]any{"per_page": perPage, "current_page": page, "next_page": nil, "prev_page": nil}
		if end < len(rows) {
			paging["next_page"] = page + 1
		}
		if query.Get("include_paging_counts") == "true" {
			paging["total_count"] = len(rows)
			paging["total_pages"] = (len(rows) + perPage - 1) / perPage
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": rows[start:end], "metadata": map[string]any{"paging": paging}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheKeyAndListThree(t *testing.T) {
	server, calls := fakeSalesloft(t)
	source := New("key-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "people" || objects[2].Label != "Cadences" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "/v2/people.json?per_page=1") {
		t.Fatalf("the key was not proved with one small call: %v", *calls)
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
	_, err = source.Describe("steps")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"status":403,"error":"Forbidden"}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "Forbidden") {
		t.Fatalf("a 403 names the error, got %v", err)
	}
}

func TestDescribeCountsAndFlattensPeopleWithCustomFields(t *testing.T) {
	server, calls := fakeSalesloft(t)
	description, err := New("key-good", server.URL).Describe("people")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "People" || description.Hash != "2023-11-15T10:00:00.123456Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["account_id"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["account_id"].Samples[0] != "10" || byName["owner_id"].Samples[0] != "7" || byName["stage_id"].Samples[0] != "2" {
		t.Fatalf("domains and references: %+v %+v", byName["email_domain"], byName["account_id"])
	}
	if byName["name"].Samples[1] != "Bo Fabrik" || byName["name"].Samples[2] != "cy@contoso.example" {
		t.Fatalf("names fall back to first and last, then the email: %v", byName["name"].Samples)
	}
	if byName["do_not_contact"].Samples[0] != "no" || byName["do_not_contact"].Samples[1] != "yes" || byName["created"].Samples[0] != "2023-11-14T22:13:20Z" || byName["tags"].Samples[0] != "finance,vip" {
		t.Fatalf("flags, times and tags: %v %v %v", byName["do_not_contact"].Samples, byName["created"].Samples, byName["tags"].Samples)
	}
	tier, ok := byName["customer_tier"]
	if !ok || tier.Filled != 1 || tier.Samples[0] != "gold" {
		t.Fatalf("the custom field is a column named as the team named it: %+v", tier)
	}
	if byName["regions"].Samples[0] != "east,west" {
		t.Fatalf("a multi-value custom field joins sorted: %+v", byName["regions"])
	}
	if _, company := byName["segment"]; company {
		t.Fatal("a company custom field is no column on people")
	}
	definitions := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v2/custom_fields.json") {
			definitions++
		}
	}
	if definitions != 1 {
		t.Fatalf("the custom field definitions read once: %v", *calls)
	}
}

func TestPageWalksNumberedPages(t *testing.T) {
	server, calls := fakeSalesloft(t)
	source := New("key-good", server.URL)
	first, err := source.Page("people", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T10:00:00.123456Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("people", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00.123456Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/v2/people.json?") && strings.Contains(call, "page=2") && strings.Contains(call, "per_page=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not ask for page 2: %v", *calls)
	}

	accounts, err := source.Page("accounts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := accounts.Rows[0]
	if row["name"] != "Northwind Traders" || row["domain"] != "northwind.example" || row["owner_id"] != "7" || row["stage_id"] != "5" || row["segment"] != "enterprise" || row["archived_at"] != "" {
		t.Fatalf("account row: %v", row)
	}
	cadences, err := source.Page("cadences", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if cadences.Rows[0]["type"] != "team" || cadences.Rows[0]["state"] != "active" || cadences.Rows[0]["shared"] != "yes" || cadences.Rows[0]["tags"] != "q4" {
		t.Fatalf("cadence row: %v", cadences.Rows[0])
	}
	if _, custom := cadences.Rows[0]["forecast"]; custom {
		t.Fatal("a cadence carries no custom fields")
	}
}

func TestDeltaFiltersAfterTheMark(t *testing.T) {
	server, calls := fakeSalesloft(t)
	source := New("key-good", server.URL)
	delta, err := source.Delta("people", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00.123456Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("people", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00.000000Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00.123456Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("accounts", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00.000000Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.Contains(call, "/v2/people.json?") && strings.Contains(call, "updated_at%5Bgt%5D=") && strings.Contains(call, "per_page=1") {
			filtered++
		}
	}
	if filtered != 2 {
		t.Fatalf("each marked delta is one filtered list: %d in %v", filtered, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.000000Z") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if columnName("Customer Tier") != "customer_tier" || columnName(" Setup fee (USD) ") != "setup_fee_usd" {
		t.Fatal("column names did not fold")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00.000000Z") != "2023-01-01T00:00:00.000000Z" || newestUpdated([]record{{"updated_at": "2022-01-01T00:00:00Z"}}, "2023-01-01T00:00:00.000000Z") != "2023-01-01T00:00:00.000000Z" {
		t.Fatal("the mark never moves backwards")
	}
	if salesloftMessage([]byte(`{"status":422,"errors":{"page":["is too large"]}}`)) != `{"page":["is too large"]}` {
		t.Fatal("a field error reads as its JSON")
	}
}
