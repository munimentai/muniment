package intercom

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

func fakeIntercom(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	contacts := []map[string]any{
		{"id": "c1", "external_id": "u-1", "role": "user", "name": "Ann Lee", "email": "Ann@Northwind.example", "phone": "+1 555 010 0000", "owner_id": 7.0,
			"companies": map[string]any{"data": []any{map[string]any{"id": "co1"}, map[string]any{"id": "co2"}}}, "location": map[string]any{"city": "Seattle", "country": "United States"},
			"unsubscribed_from_emails": false, "signed_up_at": 1700000000.0, "last_seen_at": 1700042400.0, "created_at": 1700000000.0, "updated_at": 1700042400.0,
			"custom_attributes": map[string]any{"Customer Tier": "gold", "seats": 12.0, "name": "shadow"}},
		{"id": "c2", "role": "lead", "email": "bo@fabrikam.example", "created_at": 1699990000.0, "updated_at": 1699990000.0, "custom_attributes": map[string]any{}},
		{"id": "c3", "role": "lead", "name": "Cy", "created_at": 1699980000.0, "updated_at": 1699980000.0},
	}
	companies := []map[string]any{
		{"id": "co1", "company_id": "northwind", "name": "Northwind Traders", "website": "https://www.northwind.example", "industry": "Software", "size": 120.0, "plan": map[string]any{"name": "Team"}, "monthly_spend": 490.0, "session_count": 8.0, "user_count": 7.0,
			"remote_created_at": 1690000000.0, "created_at": 1690000000.0, "updated_at": 1700046000.0, "custom_attributes": map[string]any{"region": "west"}},
	}
	conversations := []map[string]any{
		{"id": "v1", "title": "Invoice mismatch", "state": "snoozed", "priority": "priority", "read": true, "admin_assignee_id": 7.0, "team_assignee_id": nil,
			"source": map[string]any{"type": "email", "subject": "Invoice"}, "contacts": map[string]any{"contacts": []any{map[string]any{"id": "c1"}}},
			"tags": map[string]any{"tags": []any{map[string]any{"name": "billing"}, map[string]any{"name": "acme"}}}, "snoozed_until": 1700100000.0, "created_at": 1700050000.0, "updated_at": 1700050400.0},
		{"id": "v2", "state": "closed", "priority": "not_priority", "read": false, "created_at": 1700040000.0, "updated_at": 1700040400.0},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.Header.Get("Authorization") != "Bearer tok-good" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"type":"error.list","errors":[{"code":"unauthorized","message":"Access Token Invalid"}]}`))
			return
		}
		if r.Header.Get("Intercom-Version") != APIVersion {
			t.Errorf("every call names the API version: %v", r.Header)
		}
		if r.Method == http.MethodPost && strings.HasSuffix(r.URL.Path, "/search") {
			body, _ := io.ReadAll(r.Body)
			var search struct {
				Query struct {
					Value float64 `json:"value"`
				} `json:"query"`
				Pagination struct {
					PerPage int `json:"per_page"`
				} `json:"pagination"`
			}
			_ = json.Unmarshal(body, &search)
			var rows []map[string]any
			key := "data"
			if strings.HasPrefix(r.URL.Path, "/contacts") {
				rows = contacts
			} else {
				rows = conversations
				key = "conversations"
			}
			matched := []map[string]any{}
			for _, row := range rows {
				if row["updated_at"].(float64) > search.Query.Value {
					matched = append(matched, row)
				}
			}
			for i := range matched {
				for j := i + 1; j < len(matched); j++ {
					if matched[j]["updated_at"].(float64) > matched[i]["updated_at"].(float64) {
						matched[i], matched[j] = matched[j], matched[i]
					}
				}
			}
			if search.Pagination.PerPage > 0 && len(matched) > search.Pagination.PerPage {
				matched = matched[:search.Pagination.PerPage]
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"type": "list", key: matched, "total_count": len(matched)})
			return
		}
		per, _ := strconv.Atoi(r.URL.Query().Get("per_page"))
		if per <= 0 {
			per = 50
		}
		switch r.URL.Path {
		case "/contacts":
			start, _ := strconv.Atoi(r.URL.Query().Get("starting_after"))
			end := start + per
			if end > len(contacts) {
				end = len(contacts)
			}
			answer := map[string]any{"type": "list", "data": contacts[start:end], "total_count": len(contacts), "pages": map[string]any{"type": "pages", "per_page": per}}
			if end < len(contacts) {
				answer["pages"].(map[string]any)["next"] = map[string]any{"page": 2, "starting_after": strconv.Itoa(end)}
			}
			_ = json.NewEncoder(w).Encode(answer)
		case "/companies":
			page, _ := strconv.Atoi(r.URL.Query().Get("page"))
			if page <= 0 {
				page = 1
			}
			answer := map[string]any{"type": "list", "data": companies, "total_count": len(companies), "pages": map[string]any{"type": "pages", "page": page, "per_page": per, "total_pages": 1}}
			_ = json.NewEncoder(w).Encode(answer)
		case "/conversations":
			answer := map[string]any{"type": "conversation.list", "conversations": conversations, "total_count": len(conversations), "pages": map[string]any{"type": "pages", "per_page": per, "next": nil}}
			_ = json.NewEncoder(w).Encode(answer)
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"type":"error.list","errors":[{"code":"not_found","message":"Resource Not Found"}]}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsProveTheTokenAndListThree(t *testing.T) {
	server, calls := fakeIntercom(t)
	source := New("tok-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "contacts" || objects[2].Label != "Conversations" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.Contains((*calls)[0], "GET /contacts?per_page=1") {
		t.Fatalf("the token was not proved with one small call: %v", *calls)
	}
	_, err = New("tok-bad", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	if _, err := New("", server.URL).Objects(); err == nil {
		t.Fatal("an empty token is not connected")
	}
}

func TestDescribeAndPageFlattenContactsWithCustomAttributes(t *testing.T) {
	server, calls := fakeIntercom(t)
	source := New("tok-good", server.URL)
	description, err := source.Describe("contacts")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Hash != "1700042400" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["company"].Samples[0] != "co1" || byName["companies"].Samples[0] != "co1,co2" || byName["city"].Samples[0] != "Seattle" {
		t.Fatalf("contact columns: %+v", byName)
	}
	if byName["created"].Samples[0] != "2023-11-14T22:13:20Z" || byName["unsubscribed"].Samples[0] != "no" {
		t.Fatalf("times and flags: %v %v", byName["created"].Samples, byName["unsubscribed"].Samples)
	}
	if byName["customer_tier"].Samples[0] != "gold" || byName["seats"].Samples[0] != "12" {
		t.Fatalf("custom attributes read as columns named as the workspace named them: %+v %+v", byName["customer_tier"], byName["seats"])
	}
	if byName["name"].Samples[0] != "Ann Lee" {
		t.Fatal("a custom attribute never shadows a core column")
	}

	first, err := source.Page("contacts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Next == nil || first.Next.Token != "2" || first.Hash != "1700042400" {
		t.Fatalf("first page: %+v", first)
	}
	second, err := source.Page("contacts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Next != nil || second.Rows[0]["id"] != "c3" || second.Offset != 2 {
		t.Fatalf("second page: %+v", second)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "starting_after=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not pass the cursor: %v", *calls)
	}

	companies, err := source.Page("companies", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	company := companies.Rows[0]
	if company["domain"] != "northwind.example" || company["plan"] != "Team" || company["monthly_spend"] != "490" || company["region"] != "west" || companies.Next != nil {
		t.Fatalf("company row: %v next %v", company, companies.Next)
	}
	conversations, err := source.Page("conversations", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	conversation := conversations.Rows[0]
	if conversation["status"] != "pending" || conversation["priority"] != "high" || conversation["contact"] != "c1" || conversation["tags"] != "acme,billing" || conversation["channel"] != "email" || conversation["snoozed_until"] != "2023-11-16T02:00:00Z" {
		t.Fatalf("conversation row: %v", conversation)
	}
	if conversations.Rows[1]["status"] != "closed" || conversations.Rows[1]["priority"] != "" {
		t.Fatalf("closed conversation: %v", conversations.Rows[1])
	}
}

func TestDeltaSearchesWhereItCanAndReadsWhereItCannot(t *testing.T) {
	server, calls := fakeIntercom(t)
	source := New("tok-good", server.URL)
	delta, err := source.Delta("contacts", &Cursor{Hash: "1700042400"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("contacts", &Cursor{Hash: "1699999999"})
	if err != nil || delta.State != "changed" || delta.Hash != "1700042400" {
		t.Fatalf("delta: %+v %v", delta, err)
	}
	delta, err = source.Delta("conversations", nil)
	if err != nil || delta.State != "changed" || delta.Hash != "1700050400" {
		t.Fatalf("a first delta reads the newest change: %+v %v", delta, err)
	}
	delta, err = source.Delta("companies", &Cursor{Hash: "1700046000"})
	if err != nil || delta.State != "unchanged" {
		t.Fatalf("a company delta reads the first page: %+v %v", delta, err)
	}
	searches := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "POST ") {
			searches++
		}
	}
	if searches != 3 {
		t.Fatalf("contacts and conversations search, companies list: %d in %v", searches, *calls)
	}
	if nextToken(json.RawMessage(`"https://api.intercom.io/companies?per_page=50&page=3"`)) != "page:3" || nextToken(json.RawMessage(`{"starting_after":"abc"}`)) != "abc" || nextToken(nil) != "" {
		t.Fatal("the next pointer did not read in every shape")
	}
}
