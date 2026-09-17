package freshdesk

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

func fakeFreshdesk(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	tickets := []map[string]any{
		{"id": 1.0, "subject": "Invoice mismatch", "description_text": "The total disagrees.", "status": 3.0, "priority": 2.0, "type": "Problem", "source": 1.0, "requester_id": 101.0, "company_id": 501.0, "group_id": 9.0, "responder_id": 7.0,
			"requester": map[string]any{"email": "Ann@Northwind.example", "name": "Ann Lee"}, "tags": []any{"billing", "acme"}, "due_by": "2023-11-18T09:00:00Z", "created_at": "2023-11-16T08:30:00Z", "updated_at": "2023-11-16T09:00:00Z",
			"custom_fields": map[string]any{"cf_customer_tier": "gold", "cf_escalated": true, "cf_subject": "shadow"}},
		{"id": 2.0, "subject": "Login", "status": 4.0, "priority": 4.0, "source": 3.0, "requester_id": 102.0, "created_at": "2023-11-15T08:30:00Z", "updated_at": "2023-11-15T09:00:00Z", "custom_fields": map[string]any{}},
		{"id": 3.0, "subject": "Old", "status": 5.0, "priority": 1.0, "source": 42.0, "requester_id": 102.0, "created_at": "2023-11-14T08:30:00Z", "updated_at": "2023-11-14T09:00:00Z"},
	}
	contacts := []map[string]any{
		{"id": 101.0, "name": "Ann Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "job_title": "CFO", "company_id": 501.0, "active": true, "language": "en", "tags": []any{"vip"}, "created_at": "2023-01-01T00:00:00Z", "updated_at": "2023-11-15T10:00:00Z", "custom_fields": map[string]any{"cf_tier": "gold"}},
	}
	companies := []map[string]any{
		{"id": 501.0, "name": "Northwind Traders", "domains": []any{"Northwind.example", "northwind.co"}, "description": "Seattle", "health_score": "Happy", "account_tier": "Premium", "renewal_date": "2024-06-01T00:00:00Z", "created_at": "2023-01-01T00:00:00Z", "updated_at": "2023-11-15T11:00:00Z", "custom_fields": map[string]any{"region": "west"}},
	}
	data := map[string][]map[string]any{"/api/v2/tickets": tickets, "/api/v2/contacts": contacts, "/api/v2/companies": companies}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		key, pass, _ := r.BasicAuth()
		if key != "key-good" || pass != "X" {
			if key == "key-slow" {
				w.Header().Set("Retry-After", "12")
				w.WriteHeader(http.StatusTooManyRequests)
				return
			}
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"code":"invalid_credentials","message":"You have to be logged in to perform this action."}`))
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"message":"not found"}`))
			return
		}
		if r.URL.Path == "/api/v2/tickets" && r.URL.Query().Get("updated_since") == "" {
			t.Errorf("a ticket list without updated_since reads thirty days alone: %s", r.URL.RawQuery)
		}
		since := r.URL.Query().Get("updated_since")
		if since == "" {
			since = r.URL.Query().Get("_updated_since")
		}
		kept := []map[string]any{}
		for _, row := range rows {
			if since == "" || row["updated_at"].(string) >= since {
				kept = append(kept, row)
			}
		}
		if r.URL.Query().Get("order_type") == "desc" {
			for i := range kept {
				for j := i + 1; j < len(kept); j++ {
					if kept[j]["updated_at"].(string) > kept[i]["updated_at"].(string) {
						kept[i], kept[j] = kept[j], kept[i]
					}
				}
			}
		}
		per, _ := strconv.Atoi(r.URL.Query().Get("per_page"))
		if per <= 0 {
			per = 30
		}
		page, _ := strconv.Atoi(r.URL.Query().Get("page"))
		if page <= 0 {
			page = 1
		}
		start := (page - 1) * per
		if start > len(kept) {
			start = len(kept)
		}
		end := start + per
		if end > len(kept) {
			end = len(kept)
		}
		if end < len(kept) {
			w.Header().Set("Link", fmt.Sprintf(`<%s%s?per_page=%d&page=%d>; rel="next"`, "http://"+r.Host, r.URL.Path, per, page+1))
		}
		_ = json.NewEncoder(w).Encode(kept[start:end])
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(key string) string {
	body, _ := json.Marshal(map[string]string{"domain": "acme", "api_key": key})
	return string(body)
}

func TestObjectsProveTheKeyAndBuildTheHost(t *testing.T) {
	server, calls := fakeFreshdesk(t)
	source := New(packed("key-good"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "tickets" || objects[2].Label != "Companies" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "/api/v2/contacts?page=1&per_page=1") {
		t.Fatalf("the key was not proved with one small call: %v", *calls)
	}
	if live := New(packed("x"), ""); live.baseURL != "https://acme.freshdesk.com" {
		t.Fatalf("the domain builds the host: %s", live.baseURL)
	}
	_, err = New(packed("key-bad"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the API key") {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = New(packed("key-slow"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" || !strings.Contains(failure.Message, "12 seconds") {
		t.Fatalf("a rate limit names the wait, got %v", err)
	}
}

func TestDescribeAndPageFlattenTicketsWithCustomFields(t *testing.T) {
	server, calls := fakeFreshdesk(t)
	source := New(packed("key-good"), server.URL)
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
	if byName["status"].Samples[0] != "pending" || byName["priority"].Samples[0] != "medium" || byName["source"].Samples[0] != "email" || byName["requester_email"].Samples[0] != "Ann@Northwind.example" || byName["tags"].Samples[0] != "acme,billing" {
		t.Fatalf("ticket columns: %+v %+v %+v", byName["status"], byName["priority"], byName["source"])
	}
	if byName["customer_tier"].Samples[0] != "gold" || byName["escalated"].Samples[0] != "yes" {
		t.Fatalf("custom fields read without their prefix: %+v %+v", byName["customer_tier"], byName["escalated"])
	}
	if byName["subject"].Samples[0] != "Invoice mismatch" {
		t.Fatal("a custom field never shadows a core column")
	}

	first, err := source.Page("tickets", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Next == nil || first.Next.Token != "2" || first.Rows[1]["status"] != "resolved" || first.Rows[1]["priority"] != "urgent" || first.Rows[1]["source"] != "phone" {
		t.Fatalf("first page: %+v", first)
	}
	second, err := source.Page("tickets", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Next != nil || second.Offset != 2 || second.Rows[0]["status"] != "closed" || second.Rows[0]["source"] != "42" {
		t.Fatalf("second page: %+v", second)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "/api/v2/tickets?") && strings.Contains(call, "page=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not follow the Link header: %v", *calls)
	}
	contacts, err := source.Page("contacts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	contact := contacts.Rows[0]
	if contact["email_domain"] != "northwind.example" || contact["company"] != "501" || contact["active"] != "yes" || contact["tier"] != "gold" {
		t.Fatalf("contact row: %v", contact)
	}
	companies, err := source.Page("companies", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	company := companies.Rows[0]
	if company["domain"] != "northwind.example" || company["health_score"] != "Happy" || company["region"] != "west" || company["renewal_date"] != "2024-06-01T00:00:00Z" {
		t.Fatalf("company row: %v", company)
	}
}

func TestDeltaReadsSinceTheMark(t *testing.T) {
	server, calls := fakeFreshdesk(t)
	source := New(packed("key-good"), server.URL)
	delta, err := source.Delta("tickets", &Cursor{Hash: "2023-11-16T09:00:00Z"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("tickets", &Cursor{Hash: "2023-11-15T00:00:00Z"})
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("contacts", nil)
	if err != nil || delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v %v", delta, err)
	}
	delta, err = source.Delta("companies", &Cursor{Hash: "2023-11-15T11:00:00Z"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("a company delta reads the first page: %+v %v", delta, err)
	}
	since := 0
	for _, call := range *calls {
		if strings.Contains(call, "updated_since=2023-11-15T00%3A00%3A00Z") || strings.Contains(call, "_updated_since=") {
			since++
		}
	}
	if since < 1 {
		t.Fatalf("a marked delta passes the mark as updated_since: %v", *calls)
	}
}
