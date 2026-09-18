package gong

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeGong answers the workspaces, users and calls from fixtures, pages
// two records at a time by cursor, filters calls by fromDateTime, answers
// 404 to a filter that matches nothing, and refuses a wrong key.
func fakeGong(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	users := []map[string]any{
		{"id": "7", "emailAddress": "Ann@Northwind.example", "firstName": "Ann", "lastName": "Lee", "title": "AE", "phoneNumber": "+1 555 010 0000", "extension": "12", "active": true, "managerId": "9", "emailAliases": []any{"ann.lee@northwind.example"}, "created": "2023-01-01T00:00:00-08:00"},
		{"id": "8", "emailAddress": "bo@fabrikam.example", "firstName": "Bo", "lastName": "Fabrik", "active": false, "created": "2023-02-01T00:00:00Z"},
		{"id": "9", "emailAddress": "cy@contoso.example", "active": true, "created": "2023-03-01T00:00:00Z"},
	}
	recorded := []map[string]any{
		{"id": "100", "title": "Kickoff", "url": "https://app.gong.io/call?id=100", "direction": "Outbound", "system": "Zoom", "scope": "External", "media": "Video", "language": "eng", "purpose": "Demo", "duration": 1800.0, "isPrivate": false,
			"primaryUserId": "7", "workspaceId": "w1", "clientUniqueId": "z-100", "meetingUrl": "https://zoom.example/100", "scheduled": "2023-11-14T10:00:00-08:00", "started": "2023-11-14T10:01:00-08:00"},
		{"id": "101", "title": "Renewal", "direction": "Inbound", "duration": 600.0, "isPrivate": true, "primaryUserId": "8", "workspaceId": "w1", "started": "2023-11-15T09:00:00Z"},
		{"id": "102", "title": "Follow-up", "primaryUserId": "9", "workspaceId": "w1", "started": "2023-11-16T09:00:00Z"},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		user, secret, ok := r.BasicAuth()
		switch {
		case ok && user == "key-good" && secret == "secret-good":
		case ok && user == "key-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"requestId":"r1","errors":["Too many requests"]}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"requestId":"r1","errors":["Failed to authenticate"]}`))
			return
		}
		if r.URL.Path == "/v2/workspaces" {
			_ = json.NewEncoder(w).Encode(map[string]any{"requestId": "r2", "workspaces": []any{map[string]any{"id": "w1", "name": "Sales"}}})
			return
		}
		var key string
		var rows []map[string]any
		switch r.URL.Path {
		case "/v2/users":
			key, rows = "users", users
		case "/v2/calls":
			key, rows = "calls", recorded
			if since := r.URL.Query().Get("fromDateTime"); since != "" {
				kept := []map[string]any{}
				for _, row := range rows {
					if formatTime(row["started"].(string)) >= since {
						kept = append(kept, row)
					}
				}
				rows = kept
			}
			if len(rows) == 0 {
				w.WriteHeader(http.StatusNotFound)
				_, _ = w.Write([]byte(`{"requestId":"r3","errors":["No calls found corresponding to the provided filters"]}`))
				return
			}
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"requestId":"r4","errors":["Not Found"]}`))
			return
		}
		start, _ := strconv.Atoi(r.URL.Query().Get("cursor"))
		if start > len(rows) {
			start = len(rows)
		}
		end := start + 2
		if end > len(rows) {
			end = len(rows)
		}
		records := map[string]any{"totalRecords": len(rows), "currentPageSize": end - start, "currentPageNumber": start / 2}
		if end < len(rows) {
			records["cursor"] = strconv.Itoa(end)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"requestId": "r5", "records": records, key: rows[start:end]})
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

const goodSecret = `{"access_key":"key-good","access_key_secret":"secret-good","api_domain":"https://us-12345.api.gong.io"}`

func TestObjectsProveTheKeyAndListTwo(t *testing.T) {
	server, calls := fakeGong(t)
	source := New(goodSecret, server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "users" || objects[1].Label != "Calls" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.HasPrefix((*calls)[0], "/v2/workspaces?") {
		t.Fatalf("the key was not proved with one small call: %v", *calls)
	}
	if live := New(goodSecret, ""); live.base != "https://us-12345.api.gong.io" {
		t.Fatalf("an empty base URL is the secret's API domain: %s", live.base)
	}
	if live := New(`{"access_key":"k","access_key_secret":"s"}`, ""); live.base != DefaultBaseURL {
		t.Fatalf("no API domain is the shared host: %s", live.base)
	}
	_, err = New(`{"access_key":"key-bad","access_key_secret":"x"}`, server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the access key") {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = New("bare-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Connect Gong with") {
		t.Fatalf("a bare token is not a key pair, got %v", err)
	}
	_, err = New(`{"access_key":"key-slow","access_key_secret":"x"}`, server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("deals")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"requestId":"r","errors":["Missing scope api:calls:read:basic"]}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "Missing scope") {
		t.Fatalf("a 403 names the error, got %v", err)
	}
}

func TestDescribeCountsAndFlattensUsers(t *testing.T) {
	server, _ := fakeGong(t)
	description, err := New(goodSecret, server.URL).Describe("users")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Users" || len(description.Hash) != 64 {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["manager_id"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["manager_id"].Samples[0] != "9" || byName["name"].Samples[0] != "Ann Lee" || byName["email_aliases"].Samples[0] != "ann.lee@northwind.example" {
		t.Fatalf("domains, references and names: %+v %+v", byName["email_domain"], byName["name"])
	}
	if byName["active"].Samples[0] != "yes" || byName["active"].Samples[1] != "no" || byName["created"].Samples[0] != "2023-01-01T08:00:00Z" {
		t.Fatalf("flags and times: %v %v", byName["active"].Samples, byName["created"].Samples)
	}
	if byName["name"].Filled != 2 || byName["phone"].Filled != 1 {
		t.Fatalf("the first page fills two names and one phone: %+v %+v", byName["name"], byName["phone"])
	}
}

func TestPageWalksCursorPages(t *testing.T) {
	server, calls := fakeGong(t)
	source := New(goodSecret, server.URL)
	first, err := source.Page("calls", nil, 50)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T09:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["id"] != "100" || row["direction"] != "Outbound" || row["duration_seconds"] != "1800" || row["private"] != "no" || row["primary_user_id"] != "7" || row["started"] != "2023-11-14T18:01:00Z" {
		t.Fatalf("call row: %v", row)
	}
	second, err := source.Page("calls", first.Next, 50)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "102" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("the mark keeps the newest start seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if call == "/v2/calls?cursor=2" {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not carry the cursor: %v", *calls)
	}

	users, err := source.Page("users", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(users.Hash) != 64 || users.Next == nil {
		t.Fatalf("a users page carries the hash of the first page: %+v", users)
	}
	more, err := source.Page("users", users.Next, 0)
	if err != nil {
		t.Fatal(err)
	}
	if more.Hash != users.Hash || more.Rows[0]["email"] != "cy@contoso.example" || more.Rows[0]["name"] != "cy@contoso.example" {
		t.Fatalf("later pages keep the first page's hash and a name falls back to the email: %+v", more)
	}
}

func TestDeltaFiltersCallsAndHashesUsers(t *testing.T) {
	server, calls := fakeGong(t)
	source := New(goodSecret, server.URL)
	delta, err := source.Delta("calls", &Cursor{Offset: 3, Hash: "2023-11-16T09:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("calls", &Cursor{Offset: 3, Hash: "2023-11-15T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T09:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("calls", &Cursor{Offset: 3, Hash: "2024-01-01T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("a filter that matches nothing is unchanged: %+v", delta)
	}
	delta, err = source.Delta("calls", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T09:00:00Z" {
		t.Fatalf("a first delta reads the newest start on the first page: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v2/calls?fromDateTime=") {
			filtered++
		}
	}
	if filtered != 3 {
		t.Fatalf("each marked delta is one filtered list: %d in %v", filtered, *calls)
	}

	page, err := source.Page("users", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	delta, err = source.Delta("users", &Cursor{Offset: 3, Hash: page.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("the same first page is unchanged: %+v", delta)
	}
	delta, err = source.Delta("users", &Cursor{Offset: 3, Hash: "stale"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != page.Hash {
		t.Fatalf("a different hash is changed: %+v", delta)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T10:01:00-08:00") != "2023-11-14T18:01:00Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339 in UTC")
	}
	if newestTime(nil, "started", "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" || newestTime([]record{{"started": "2022-01-01T00:00:00Z"}}, "started", "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
	if hashRecords(nil) != hashRecords([]record{}) || hashRecords([]record{{"id": "1"}}) == hashRecords([]record{{"id": "2"}}) {
		t.Fatal("the hash reads the records")
	}
	if gongMessage([]byte(`{"requestId":"r","errors":["one","two"]}`)) != "one; two" || gongMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a failure reads its errors")
	}
}
