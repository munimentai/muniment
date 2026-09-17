package zendesk

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

func fakeZendesk(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	tickets := []map[string]any{
		{"id": 1.0, "subject": "Invoice mismatch", "description": "The total disagrees.", "status": "pending", "priority": "normal", "type": "problem", "requester_id": 101.0, "assignee_id": 7.0, "organization_id": 501.0,
			"via": map[string]any{"channel": "email"}, "tags": []any{"billing", "acme"}, "created_at": "2023-11-16T08:30:00Z", "updated_at": "2023-11-16T09:00:00Z",
			"custom_fields": []any{map[string]any{"id": 9001.0, "value": "gold"}, map[string]any{"id": 9002.0, "value": true}}},
		{"id": 2.0, "subject": "Login", "status": "solved", "priority": "urgent", "requester_id": 102.0, "created_at": "2023-11-15T08:30:00Z", "updated_at": "2023-11-15T09:00:00Z", "custom_fields": []any{}},
		{"id": 3.0, "subject": "Old", "status": "closed", "priority": "low", "requester_id": 102.0, "created_at": "2023-11-14T08:30:00Z", "updated_at": "2023-11-14T09:00:00Z"},
	}
	users := []map[string]any{
		{"id": 101.0, "name": "Ann Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "role": "end-user", "organization_id": 501.0, "active": true, "verified": true, "time_zone": "Pacific Time (US & Canada)",
			"tags": []any{"vip"}, "created_at": "2023-01-01T00:00:00Z", "updated_at": "2023-11-15T10:00:00Z", "user_fields": map[string]any{"tier": "gold", "seats": 12.0}},
	}
	organizations := []map[string]any{
		{"id": 501.0, "name": "Northwind Traders", "domain_names": []any{"Northwind.example", "northwind.co"}, "details": "Seattle", "tags": []any{}, "created_at": "2023-01-01T00:00:00Z", "updated_at": "2023-11-15T11:00:00Z", "organization_fields": map[string]any{"region": "west"}},
	}
	data := map[string][]map[string]any{"/api/v2/tickets": tickets, "/api/v2/users": users, "/api/v2/organizations": organizations}
	keys := map[string]string{"/api/v2/tickets": "tickets", "/api/v2/users": "users", "/api/v2/organizations": "organizations"}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		user, pass, _ := r.BasicAuth()
		if user != "ann@northwind.example/token" || pass != "tok-good" {
			if pass == "tok-slow" {
				w.Header().Set("Retry-After", "37")
				w.WriteHeader(http.StatusTooManyRequests)
				return
			}
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":"Couldn't authenticate you"}`))
			return
		}
		switch r.URL.Path {
		case "/api/v2/ticket_fields":
			_ = json.NewEncoder(w).Encode(map[string]any{"ticket_fields": []any{
				map[string]any{"id": 1.0, "title": "Subject", "type": "subject", "removable": false, "active": true},
				map[string]any{"id": 9001.0, "title": "Customer Tier", "type": "tagger", "removable": true, "active": true},
				map[string]any{"id": 9002.0, "title": "Escalated", "type": "checkbox", "removable": true, "active": true},
				map[string]any{"id": 9003.0, "title": "Retired", "type": "text", "removable": true, "active": false},
			}})
			return
		case "/api/v2/user_fields":
			_ = json.NewEncoder(w).Encode(map[string]any{"user_fields": []any{
				map[string]any{"id": 1.0, "key": "tier", "title": "Tier", "type": "dropdown", "active": true},
				map[string]any{"id": 2.0, "key": "seats", "title": "Seats", "type": "integer", "active": true},
			}})
			return
		case "/api/v2/organization_fields":
			_ = json.NewEncoder(w).Encode(map[string]any{"organization_fields": []any{map[string]any{"id": 3.0, "key": "region", "title": "Region", "type": "text", "active": true}}})
			return
		case "/api/v2/search":
			term := r.URL.Query().Get("query")
			path := map[string]string{"ticket": "/api/v2/tickets", "user": "/api/v2/users", "organization": "/api/v2/organizations"}[strings.TrimPrefix(strings.Fields(term)[0], "type:")]
			since := ""
			for _, part := range strings.Fields(term) {
				if strings.HasPrefix(part, "updated>") {
					since = strings.TrimPrefix(part, "updated>")
				}
			}
			results := []map[string]any{}
			for _, row := range data[path] {
				if row["updated_at"].(string) > since {
					results = append(results, row)
				}
			}
			for i := range results {
				for j := i + 1; j < len(results); j++ {
					if results[j]["updated_at"].(string) > results[i]["updated_at"].(string) {
						results[i], results[j] = results[j], results[i]
					}
				}
			}
			if len(results) > 1 {
				results = results[:1]
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"results": results, "count": len(results)})
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":"RecordNotFound","description":"Not found"}`))
			return
		}
		size, _ := strconv.Atoi(r.URL.Query().Get("page[size]"))
		if size <= 0 {
			size = 100
		}
		start, _ := strconv.Atoi(r.URL.Query().Get("page[after]"))
		end := start + size
		if end > len(rows) {
			end = len(rows)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{keys[r.URL.Path]: rows[start:end], "meta": map[string]any{"has_more": end < len(rows), "after_cursor": strconv.Itoa(end)}})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(token string) string {
	body, _ := json.Marshal(map[string]string{"subdomain": "acme", "email": "ann@northwind.example", "api_token": token})
	return string(body)
}

func TestObjectsProveTheTokenAndBuildTheHost(t *testing.T) {
	server, calls := fakeZendesk(t)
	source := New(packed("tok-good"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "tickets" || objects[2].Label != "Organizations" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "/api/v2/users?page%5Bsize%5D=1") {
		t.Fatalf("the token was not proved with one small call: %v", *calls)
	}
	if live := New(packed("x"), ""); live.baseURL != "https://acme.zendesk.com" {
		t.Fatalf("the subdomain builds the host: %s", live.baseURL)
	}
	if live := New(`{"subdomain":"https://acme.zendesk.com","email":"a","api_token":"b"}`, ""); live.baseURL != "https://acme.zendesk.com" {
		t.Fatalf("a pasted host reads as its subdomain: %s", live.baseURL)
	}
	_, err = New(packed("tok-bad"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "email and API token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New(packed("tok-slow"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" || !strings.Contains(failure.Message, "37 seconds") {
		t.Fatalf("a rate limit names the wait, got %v", err)
	}
	_, err = New("bare", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" {
		t.Fatalf("one bare value names the three credentials, got %v", err)
	}
}

func TestDescribeAndPageFlattenTicketsWithCustomFields(t *testing.T) {
	server, calls := fakeZendesk(t)
	source := New(packed("tok-good"), server.URL)
	description, err := source.Describe("tickets")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["requester"].Guess != "id" || byName["status"].Samples[0] != "pending" || byName["priority"].Samples[0] != "medium" || byName["tags"].Samples[0] != "acme,billing" {
		t.Fatalf("ticket columns: %+v %+v %+v", byName["status"], byName["priority"], byName["tags"])
	}
	tier, ok := byName["customer_tier"]
	if !ok || tier.Samples[0] != "gold" || byName["escalated"].Guess != "boolean" || byName["escalated"].Samples[0] != "yes" {
		t.Fatalf("ticket custom fields read by id: %+v %+v", tier, byName["escalated"])
	}
	if _, retired := byName["retired"]; retired {
		t.Fatal("an inactive field must not become a column")
	}
	if _, subject := byName["subject_2"]; subject {
		t.Fatal("a system field must not become a column")
	}

	first, err := source.Page("tickets", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Next == nil || first.Next.Token != "2" || first.Rows[1]["status"] != "resolved" || first.Rows[1]["priority"] != "urgent" {
		t.Fatalf("first page: %+v", first)
	}
	second, err := source.Page("tickets", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Next != nil || second.Offset != 2 || second.Rows[0]["status"] != "closed" || second.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("second page: %+v", second)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "page%5Bafter%5D=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not pass the cursor: %v", *calls)
	}
	fieldReads := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/api/v2/ticket_fields") {
			fieldReads++
		}
	}
	if fieldReads != 1 {
		t.Fatalf("the ticket fields read once per source: %d", fieldReads)
	}

	users, err := source.Page("users", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	user := users.Rows[0]
	if user["email_domain"] != "northwind.example" || user["organization"] != "501" || user["active"] != "yes" || user["tier"] != "gold" || user["seats"] != "12" || user["tags"] != "vip" {
		t.Fatalf("user row: %v", user)
	}
	organizations, err := source.Page("organizations", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	org := organizations.Rows[0]
	if org["domain"] != "northwind.example" || org["domains"] != "Northwind.example,northwind.co" || org["region"] != "west" {
		t.Fatalf("organization row: %v", org)
	}
}

func TestDeltaSearchesOnce(t *testing.T) {
	server, calls := fakeZendesk(t)
	source := New(packed("tok-good"), server.URL)
	delta, err := source.Delta("tickets", &Cursor{Hash: "2023-11-16T09:00:00Z"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("tickets", &Cursor{Hash: "2023-11-15T00:00:00Z"})
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("users", nil)
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v %v", delta, err)
	}
	searches := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/api/v2/search?") {
			searches++
			if !strings.Contains(call, "per_page=1") || !strings.Contains(call, "sort_order=desc") {
				t.Fatalf("a delta search asks for one newest result: %s", call)
			}
		}
	}
	if searches != 3 {
		t.Fatalf("each delta is one search: %d", searches)
	}
}
