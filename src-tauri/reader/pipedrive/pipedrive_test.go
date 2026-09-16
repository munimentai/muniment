package pipedrive

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakePipedrive answers the three lists and their field endpoints from
// fixtures, pages by start, answers recents and stages, and refuses a
// wrong token.
func fakePipedrive(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	persons := []map[string]any{
		{"id": 1.0, "name": "Ann Lee", "first_name": "Ann", "last_name": "Lee", "job_title": "CFO", "label": 3.0, "open_deals_count": 1.0, "active_flag": true,
			"email":  []any{map[string]any{"value": "old@northwind.example", "primary": false}, map[string]any{"value": "Ann@Northwind.example", "primary": true}},
			"phone":  []any{map[string]any{"value": "+1 555 010 0000", "primary": true}},
			"org_id": map[string]any{"value": 10.0, "name": "Northwind Traders"}, "owner_id": map[string]any{"id": 7.0, "name": "Mikey"},
			"add_time": "2023-11-14 22:13:20", "update_time": "2023-11-15 10:00:00",
			"a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4": "gold"},
		{"id": 2.0, "name": "Bo Fabrik", "email": []any{map[string]any{"value": "bo@fabrikam.example", "primary": false}}, "phone": []any{}, "org_id": nil, "active_flag": true,
			"add_time": "2023-11-13 09:00:00", "update_time": "2023-11-13 09:00:00"},
		{"id": 3.0, "name": "Cy Contoso", "email": []any{}, "active_flag": false, "add_time": "2023-11-12 09:00:00", "update_time": "2023-11-12 09:00:00"},
	}
	organizations := []map[string]any{
		{"id": 10.0, "name": "Northwind Traders", "address": "1 Pier, Seattle", "address_locality": "Seattle", "address_country": "US", "people_count": 7.0, "open_deals_count": 1.0, "won_deals_count": 2.0,
			"owner_id": map[string]any{"id": 7.0, "name": "Mikey"}, "active_flag": true, "add_time": "2023-01-01 00:00:00", "update_time": "2023-11-15 11:00:00"},
	}
	deals := []map[string]any{
		{"id": 100.0, "title": "Northwind renewal", "value": 96000.0, "currency": "USD", "status": "open", "stage_id": 4.0, "pipeline_id": 1.0,
			"person_id": map[string]any{"value": 1.0, "name": "Ann Lee"}, "org_id": map[string]any{"value": 10.0, "name": "Northwind Traders"}, "user_id": map[string]any{"id": 7.0, "name": "Mikey"},
			"expected_close_date": "2023-12-16", "won_time": nil, "lost_time": nil, "active": true, "add_time": "2023-10-01 00:00:00", "update_time": "2023-11-16 08:00:00",
			"b1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4": map[string]any{"value": 1200.0, "currency": "USD"}},
		{"id": 101.0, "title": "Contoso pilot", "value": 1200.0, "currency": "USD", "status": "won", "stage_id": 4.0, "pipeline_id": 1.0, "won_time": "2023-11-10 08:00:00", "active": false, "add_time": "2023-09-01 00:00:00", "update_time": "2023-11-10 08:00:00"},
		{"id": 102.0, "title": "Fabrikam", "status": "lost", "stage_id": 2.0, "pipeline_id": 1.0, "lost_time": "2023-11-09 08:00:00", "lost_reason": "No budget", "active": false, "add_time": "2023-09-02 00:00:00", "update_time": "2023-11-09 08:00:00"},
		{"id": 103.0, "title": "Early", "status": "open", "stage_id": 1.0, "pipeline_id": 1.0, "active": true, "add_time": "2023-09-03 00:00:00", "update_time": "2023-11-08 08:00:00"},
		{"id": 104.0, "title": "Middle", "status": "open", "stage_id": 2.0, "pipeline_id": 1.0, "active": true, "add_time": "2023-09-04 00:00:00", "update_time": "2023-11-07 08:00:00"},
		{"id": 105.0, "title": "Later", "status": "open", "stage_id": 3.0, "pipeline_id": 1.0, "active": true, "add_time": "2023-09-05 00:00:00", "update_time": "2023-11-06 08:00:00"},
	}
	data := map[string][]map[string]any{"/v1/persons": persons, "/v1/organizations": organizations, "/v1/deals": deals}
	recentItem := map[string]string{"person": "/v1/persons", "organization": "/v1/organizations", "deal": "/v1/deals"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		if strings.Contains(r.URL.RawQuery, "api_token") {
			t.Errorf("the token must never travel in the URL: %s", r.URL.RawQuery)
		}
		switch r.Header.Get("x-api-token") {
		case "tok-good":
		case "tok-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"success":false,"error":"Rate limit exceeded"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"success":false,"error":"unauthorized access","errorCode":401}`))
			return
		}
		switch r.URL.Path {
		case "/v1/personFields":
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": []any{
				map[string]any{"key": "name", "name": "Name", "field_type": "varchar", "edit_flag": false},
				map[string]any{"key": "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4", "name": "Customer Tier", "field_type": "enum", "edit_flag": true},
			}})
			return
		case "/v1/dealFields":
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": []any{
				map[string]any{"key": "b1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4", "name": "Setup fee", "field_type": "monetary", "edit_flag": true},
			}})
			return
		case "/v1/organizationFields":
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": []any{}})
			return
		case "/v1/stages":
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": []any{
				map[string]any{"id": 3.0, "name": "Proposal made", "pipeline_id": 1.0, "order_nr": 3.0},
				map[string]any{"id": 1.0, "name": "Lead in", "pipeline_id": 1.0, "order_nr": 1.0},
				map[string]any{"id": 2.0, "name": "Contact made", "pipeline_id": 1.0, "order_nr": 2.0},
				map[string]any{"id": 4.0, "name": "Negotiations", "pipeline_id": 1.0, "order_nr": 4.0},
				map[string]any{"id": 9.0, "name": "Only", "pipeline_id": 2.0, "order_nr": 1.0},
			}})
			return
		case "/v1/recents":
			since := r.URL.Query().Get("since_timestamp")
			path, ok := recentItem[r.URL.Query().Get("items")]
			if !ok {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			changed := []any{}
			for _, row := range data[path] {
				if row["update_time"].(string) > since {
					changed = append(changed, map[string]any{"item": r.URL.Query().Get("items"), "id": row["id"], "data": row})
				}
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": changed})
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"success":false,"error":"not found"}`))
			return
		}
		if r.URL.Query().Get("sort") == "update_time DESC" {
			sorted := append([]map[string]any(nil), rows...)
			for i := 0; i < len(sorted); i++ {
				for j := i + 1; j < len(sorted); j++ {
					if sorted[j]["update_time"].(string) > sorted[i]["update_time"].(string) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
		if limit <= 0 {
			limit = 100
		}
		start, _ := strconv.Atoi(r.URL.Query().Get("start"))
		end := start + limit
		if end > len(rows) {
			end = len(rows)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": rows[start:end], "additional_data": map[string]any{
			"pagination": map[string]any{"start": start, "limit": limit, "more_items_in_collection": end < len(rows), "next_start": end},
		}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheTokenAndListThree(t *testing.T) {
	server, calls := fakePipedrive(t)
	source := New("tok-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "persons" || objects[2].Label != "Deals" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "/v1/persons?limit=1") {
		t.Fatalf("the token was not proved with one small call: %v", *calls)
	}
	_, err = New("tok-bad", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the API token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New("tok-slow", server.URL).Objects()
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("activities")
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestDescribeFlattensPeopleWithCustomFields(t *testing.T) {
	server, _ := fakePipedrive(t)
	description, err := New("tok-good", server.URL).Describe("persons")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "People" || description.Hash != "2023-11-15 10:00:00" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["organization"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email"].Samples; len(got) != 2 || got[0] != "Ann@Northwind.example" || got[1] != "bo@fabrikam.example" {
		t.Fatalf("the primary email leads, else the first: %v", got)
	}
	if got := byName["email_domain"].Samples; got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["organization"].Samples[0] != "10" || byName["organization_name"].Samples[0] != "Northwind Traders" || byName["owner"].Samples[0] != "Mikey" {
		t.Fatalf("references: %+v %+v", byName["organization"], byName["owner"])
	}
	if byName["created"].Samples[0] != "2023-11-14T22:13:20Z" || byName["active"].Samples[1] != "no" {
		t.Fatalf("times and flags: %v %v", byName["created"].Samples, byName["active"].Samples)
	}
	tier, ok := byName["customer_tier"]
	if !ok || tier.Guess != "string" || tier.Filled != 1 || tier.Samples[0] != "gold" {
		t.Fatalf("the custom field is a column named as the company named it: %+v", tier)
	}
	if _, raw := byName["a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4"]; raw {
		t.Fatal("the raw field key must not be a column")
	}
}

func TestPageWalksStartAndFoldsDeals(t *testing.T) {
	server, calls := fakePipedrive(t)
	source := New("tok-good", server.URL)
	first, err := source.Page("persons", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15 10:00:00" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("persons", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15 10:00:00" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/v1/persons?") && strings.Contains(call, "start=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not start at 2: %v", *calls)
	}

	deals, err := source.Page("deals", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := deals.Rows[0]
	if row["name"] != "Northwind renewal" || row["amount"] != "96000" || row["status"] != "open" || row["stage_name"] != "Negotiations" || row["stage"] != "negotiation" {
		t.Fatalf("deal row: %v", row)
	}
	if row["person"] != "1" || row["organization"] != "10" || row["expected_close"] != "2023-12-16" || row["owner"] != "Mikey" || row["setup_fee"] != "1200" {
		t.Fatalf("deal references, dates and the monetary custom field: %v", row)
	}
	stages := []string{}
	for _, entry := range deals.Rows {
		stages = append(stages, entry["stage"])
	}
	if strings.Join(stages, ",") != "negotiation,won,lost,discovery,qualification,proposal" {
		t.Fatalf("stages fold by status, then by position in the pipeline: %v", stages)
	}
	if deals.Rows[2]["lost_reason"] != "No budget" || deals.Rows[2]["lost_at"] != "2023-11-09T08:00:00Z" || deals.Rows[1]["won_at"] != "2023-11-10T08:00:00Z" {
		t.Fatalf("won and lost times: %v %v", deals.Rows[1], deals.Rows[2])
	}
	stageCalls := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v1/stages") {
			stageCalls++
		}
	}
	if stageCalls != 1 {
		t.Fatalf("the stages read once per source: %d", stageCalls)
	}
}

func TestDeltaReadsRecentsAfterTheMark(t *testing.T) {
	server, calls := fakePipedrive(t)
	source := New("tok-good", server.URL)
	delta, err := source.Delta("persons", &Cursor{Offset: 3, Hash: "2023-11-15 10:00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("persons", &Cursor{Offset: 3, Hash: "2023-11-13 00:00:00"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15 10:00:00" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("deals", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16 08:00:00" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	recents := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v1/recents?") && strings.Contains(call, "items=person") {
			recents++
		}
	}
	if recents != 2 {
		t.Fatalf("each marked delta is one recents call: %d in %v", recents, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14 22:13:20") != "2023-11-14T22:13:20Z" || formatTime("0000-00-00 00:00:00") != "" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if columnName("Customer Tier") != "customer_tier" || columnName(" Setup fee (USD) ") != "setup_fee_usd" {
		t.Fatal("column names did not fold")
	}
	if customValue(map[string]any{"value": 12.5, "currency": "USD"}) != "12.5" || customValue([]any{"a", "b"}) != "a,b" || customValue(nil) != "" {
		t.Fatal("custom values did not read")
	}
	if newestUpdate(nil, "2023-01-01 00:00:00") != "2023-01-01 00:00:00" || newestUpdate([]item{{"update_time": "2022-01-01 00:00:00"}}, "2023-01-01 00:00:00") != "2023-01-01 00:00:00" {
		t.Fatal("the mark never moves backwards")
	}
	if guessType("monetary") != "number" || guessType("date") != "date" || guessType("enum") != "string" {
		t.Fatal("field types did not guess")
	}
}
