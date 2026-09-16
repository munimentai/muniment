package hubspot

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeHubSpot answers the four list endpoints and their properties from
// fixtures, pages by `after`, answers one search per object, refuses a
// wrong token, and rate limits by policy for two special tokens.
func fakeHubSpot(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	contacts := []map[string]any{
		{"id": "101", "createdAt": "2023-11-14T22:13:20.000Z", "updatedAt": "2023-11-15T10:00:00.000Z", "archived": false,
			"properties": map[string]any{"firstname": "Ann", "lastname": "Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "jobtitle": "CFO",
				"company": "Northwind Traders", "lifecyclestage": "customer", "hs_lead_status": "OPEN", "hubspot_owner_id": "7",
				"createdate": "2023-11-14T22:13:20.000Z", "lastmodifieddate": "2023-11-15T10:00:00.000Z", "renewal_risk": "0.2"},
			"associations": map[string]any{"companies": map[string]any{"results": []any{
				map[string]any{"id": "501", "type": "contact_to_company"}, map[string]any{"id": "501", "type": "contact_to_company_unlabeled"},
			}}}},
		{"id": "102", "createdAt": "2023-11-14T22:15:00.000Z", "updatedAt": "2023-11-14T22:15:00.000Z", "archived": false,
			"properties": map[string]any{"firstname": "", "lastname": "", "email": "ops@contoso.example", "createdate": "2023-11-14T22:15:00.000Z", "lastmodifieddate": "2023-11-14T22:15:00.000Z"}},
		{"id": "103", "createdAt": "2023-11-13T09:00:00.000Z", "updatedAt": "2023-11-13T09:00:00.000Z", "archived": false,
			"properties": map[string]any{"firstname": "Bo", "lastname": "Fabrik", "email": nil, "createdate": "2023-11-13T09:00:00.000Z", "lastmodifieddate": "2023-11-13T09:00:00.000Z", "renewal_risk": "0.9"}},
	}
	companies := []map[string]any{
		{"id": "501", "updatedAt": "2023-11-15T11:00:00.000Z", "archived": false,
			"properties": map[string]any{"name": "Northwind Traders", "domain": "northwind.example", "industry": "COMPUTER_SOFTWARE", "numberofemployees": "120", "annualrevenue": "4500000",
				"createdate": "2023-01-01T00:00:00.000Z", "hs_lastmodifieddate": "2023-11-15T11:00:00.000Z"}},
	}
	deals := []map[string]any{
		{"id": "901", "updatedAt": "2023-11-16T08:00:00.000Z", "archived": false,
			"properties": map[string]any{"dealname": "Northwind renewal", "amount": "96000", "deal_currency_code": "USD", "pipeline": "default", "dealstage": "contractsent",
				"hs_is_closed": "false", "hs_is_closed_won": "false", "closedate": "2023-12-16T00:00:00.000Z", "createdate": "2023-10-01T00:00:00.000Z", "hs_lastmodifieddate": "2023-11-16T08:00:00.000Z"},
			"associations": map[string]any{
				"companies": map[string]any{"results": []any{map[string]any{"id": "501"}}},
				"contacts":  map[string]any{"results": []any{map[string]any{"id": "101"}, map[string]any{"id": "102"}}},
			}},
		{"id": "902", "updatedAt": "2023-11-10T08:00:00.000Z", "archived": false,
			"properties": map[string]any{"dealname": "Contoso pilot", "amount": "1200", "pipeline": "custom", "dealstage": "9001", "hs_is_closed": "true", "hs_is_closed_won": "true", "hs_lastmodifieddate": "2023-11-10T08:00:00.000Z"}},
		{"id": "903", "updatedAt": "2023-11-09T08:00:00.000Z", "archived": false,
			"properties": map[string]any{"dealname": "Fabrikam", "pipeline": "custom", "dealstage": "9002", "hs_is_closed": "true", "hs_is_closed_won": "false", "hs_lastmodifieddate": "2023-11-09T08:00:00.000Z"}},
	}
	tickets := []map[string]any{
		{"id": "701", "updatedAt": "2023-11-16T09:00:00.000Z", "archived": false,
			"properties": map[string]any{"subject": "Invoice mismatch", "content": "The total disagrees with the quote.", "hs_pipeline": "0", "hs_pipeline_stage": "2", "hs_ticket_priority": "HIGH",
				"createdate": "2023-11-16T08:30:00.000Z", "hs_lastmodifieddate": "2023-11-16T09:00:00.000Z"},
			"associations": map[string]any{"contacts": map[string]any{"results": []any{map[string]any{"id": "101"}}}}},
	}
	data := map[string][]map[string]any{"contacts": contacts, "companies": companies, "deals": deals, "tickets": tickets}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		token := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		switch token {
		case "pat-good", "pat-noscope":
		case "pat-daily":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"status":"error","message":"You have reached your daily limit.","category":"RATE_LIMITS","policyName":"DAILY"}`))
			return
		case "pat-burst":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"status":"error","message":"You have reached your secondly limit.","category":"RATE_LIMITS","policyName":"SECONDLY"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"status":"error","message":"Authentication credentials not found.","category":"INVALID_AUTHENTICATION"}`))
			return
		}
		parts := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
		if len(parts) == 4 && parts[2] == "properties" {
			switch parts[3] {
			case "contacts":
				_ = json.NewEncoder(w).Encode(map[string]any{"results": []any{
					map[string]any{"name": "renewal_risk", "type": "number", "hubspotDefined": false, "hidden": false},
					map[string]any{"name": "secret_note", "type": "string", "hubspotDefined": false, "hidden": true},
					map[string]any{"name": "firstname", "type": "string", "hubspotDefined": true, "hidden": false},
					map[string]any{"name": "email", "type": "string", "hubspotDefined": false, "hidden": false},
				}})
			case "deals":
				w.WriteHeader(http.StatusForbidden)
				_, _ = w.Write([]byte(`{"status":"error","message":"This app hasn't been granted all required scopes"}`))
			default:
				_ = json.NewEncoder(w).Encode(map[string]any{"results": []any{}})
			}
			return
		}
		if len(parts) < 4 || parts[2] != "objects" {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"message":"no such route"}`))
			return
		}
		name := parts[3]
		rows := data[name]
		if token == "pat-noscope" && name == "deals" {
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"status":"error","message":"This app hasn't been granted all required scopes"}`))
			return
		}
		if len(parts) == 5 && parts[4] == "search" && r.Method == http.MethodPost {
			body, _ := io.ReadAll(r.Body)
			var search struct {
				FilterGroups []struct {
					Filters []struct {
						PropertyName, Operator, Value string
					}
				} `json:"filterGroups"`
				Limit int `json:"limit"`
			}
			_ = json.Unmarshal(body, &search)
			matched := []map[string]any{}
			for _, row := range rows {
				keep := true
				for _, group := range search.FilterGroups {
					for _, filter := range group.Filters {
						after, _ := strconv.ParseInt(filter.Value, 10, 64)
						if filter.Operator == "GT" && modifiedMillis(row) <= after {
							keep = false
						}
					}
				}
				if keep {
					matched = append(matched, row)
				}
			}
			// Newest first, as the sort asks.
			for i := 0; i < len(matched); i++ {
				for j := i + 1; j < len(matched); j++ {
					if modifiedMillis(matched[j]) > modifiedMillis(matched[i]) {
						matched[i], matched[j] = matched[j], matched[i]
					}
				}
			}
			limit := search.Limit
			if limit <= 0 || limit > len(matched) {
				limit = len(matched)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"total": len(matched), "results": matched[:limit]})
			return
		}
		if r.Method != http.MethodGet {
			w.WriteHeader(http.StatusMethodNotAllowed)
			return
		}
		limit := 100
		if text := r.URL.Query().Get("limit"); text != "" {
			limit, _ = strconv.Atoi(text)
		}
		start := 0
		if after := r.URL.Query().Get("after"); after != "" {
			start, _ = strconv.Atoi(after)
		}
		end := start + limit
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{"results": rows[start:end]}
		if end < len(rows) {
			answer["paging"] = map[string]any{"next": map[string]any{"after": strconv.Itoa(end)}}
		}
		_ = json.NewEncoder(w).Encode(answer)
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheTokenAndListFour(t *testing.T) {
	server, calls := fakeHubSpot(t)
	source := New("pat-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "contacts" || objects[3].Label != "Tickets" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "GET /crm/v3/objects/contacts?limit=1") {
		t.Fatalf("the token was not proved with one small call: %v", *calls)
	}

	_, err = New("pat-bad", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = source.Describe("quotes")
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestDescribeFlattensContactsWithCustomProperties(t *testing.T) {
	server, calls := fakeHubSpot(t)
	description, err := New("pat-good", server.URL).Describe("contacts")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Contacts" || description.Hash != "1700042400000" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["company"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["name"].Samples; len(got) != 3 || got[0] != "Ann Lee" || got[1] != "ops@contoso.example" {
		t.Fatalf("names fall back to the email: %v", got)
	}
	if got := byName["email_domain"].Samples; len(got) != 2 || got[0] != "northwind.example" {
		t.Fatalf("email domains: %v", got)
	}
	if byName["email"].Filled != 2 || byName["company"].Filled != 1 || byName["company"].Samples[0] != "501" {
		t.Fatalf("filled counts and the first company: %+v", byName["company"])
	}
	if got := byName["created"].Samples[0]; got != "2023-11-14T22:13:20Z" {
		t.Fatalf("created: %s", got)
	}
	custom, ok := byName["renewal_risk"]
	if !ok || custom.Guess != "number" || custom.Filled != 2 || custom.Samples[0] != "0.2" {
		t.Fatalf("the custom property is its own column: %+v", custom)
	}
	if _, hidden := byName["secret_note"]; hidden {
		t.Fatal("a hidden property must not become a column")
	}
	if _, defined := byName["firstname"]; defined {
		t.Fatal("a HubSpot-defined property must not repeat a core column")
	}
	listCall := ""
	for _, call := range *calls {
		if strings.Contains(call, "/objects/contacts?") {
			listCall = call
		}
	}
	for _, need := range []string{"properties=", "renewal_risk", "lastmodifieddate", "associations=companies", "archived=false"} {
		if !strings.Contains(listCall, need) {
			t.Fatalf("the list call lacks %s: %s", need, listCall)
		}
	}
	if strings.Contains(listCall, "search") {
		t.Fatal("a backfill must never spend the search budget")
	}
}

func TestPageWalksAfterAndFoldsDealsAndTickets(t *testing.T) {
	server, calls := fakeHubSpot(t)
	source := New("pat-good", server.URL)
	first, err := source.Page("contacts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "1700042400000" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	if first.Rows[0]["company"] != "501" || first.Rows[0]["job_title"] != "CFO" || first.Rows[0]["archived"] != "no" {
		t.Fatalf("contact row: %v", first.Rows[0])
	}
	second, err := source.Page("contacts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "103" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "1700042400000" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/objects/contacts?") && strings.Contains(call, "after=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not start after 2: %v", *calls)
	}

	deals, err := source.Page("deals", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := deals.Rows[0]
	if row["name"] != "Northwind renewal" || row["amount"] != "96000" || row["stage"] != "negotiation" || row["stage_id"] != "contractsent" || row["closed"] != "no" {
		t.Fatalf("deal row: %v", row)
	}
	if row["company"] != "501" || row["contacts"] != "101,102" || row["close_date"] != "2023-12-16T00:00:00Z" {
		t.Fatalf("deal associations and dates: %v", row)
	}
	if deals.Rows[1]["stage"] != "won" || deals.Rows[2]["stage"] != "lost" {
		t.Fatalf("a custom pipeline folds by closed and won: %v %v", deals.Rows[1], deals.Rows[2])
	}
	if _, extra := row["renewal_risk"]; extra {
		t.Fatal("a scope refusal on the properties read must leave the core columns alone")
	}

	tickets, err := source.Page("tickets", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	ticket := tickets.Rows[0]
	if ticket["subject"] != "Invoice mismatch" || ticket["status"] != "pending" || ticket["priority"] != "high" || ticket["contacts"] != "101" || ticket["company"] != "" {
		t.Fatalf("ticket row: %v", ticket)
	}
}

func TestDeltaSearchesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeHubSpot(t)
	source := New("pat-good", server.URL)
	delta, err := source.Delta("contacts", &Cursor{Offset: 3, Hash: "1700042400000"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("contacts", &Cursor{Offset: 3, Hash: "1699999999000"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "1700042400000" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("deals", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "1700121600000" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	searches := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "POST ") && strings.Contains(call, "/search") {
			searches++
		}
	}
	if searches != 3 {
		t.Fatalf("each delta is one search: %d in %v", searches, *calls)
	}
}

func TestRefusalsNameTheirCause(t *testing.T) {
	server, _ := fakeHubSpot(t)
	_, err := New("pat-daily", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "rate_limited" || !strings.Contains(failure.Message, "tomorrow") {
		t.Fatalf("a daily limit waits a day: %v", err)
	}
	_, err = New("pat-burst", server.URL).Objects()
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "rate_limited" || !strings.Contains(failure.Message, "minute") {
		t.Fatalf("a secondly limit waits a minute: %v", err)
	}
	_, err = New("pat-noscope", server.URL).Page("deals", nil, 0)
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "crm.objects.deals.read") {
		t.Fatalf("a scope refusal names the scope: %v", err)
	}
}

func TestTimesAndMarksRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.123Z") != "2023-11-14T22:13:20Z" || formatTime("1700000000000") != "2023-11-14T22:13:20Z" || formatTime("2023-11-14") != "2023-11-14T00:00:00Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if scalar(3.0) != "3" || scalar(2.5) != "2.5" || scalar(true) != "yes" || scalar(nil) != "" {
		t.Fatal("scalars did not read as text")
	}
	if newestModified(nil, "42") != "42" || newestModified([]record{{"updatedAt": "1970-01-01T00:00:00.007Z"}}, "42") != "42" {
		t.Fatal("the mark never moves backwards")
	}
	if guessType("datetime") != "date-time" || guessType("bool") != "boolean" || guessType("enumeration") != "string" {
		t.Fatal("property types did not guess")
	}
	if objectOf("/crm/v3/objects/tickets/search") != "tickets" || objectOf("/crm/v3/properties/deals") != "the object" {
		t.Fatal("the object did not read from the path")
	}
}
