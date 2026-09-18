package kit

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"testing"
	"time"
)

// fixtures is the fake account: the lists a test may change between calls.
type fixtures struct {
	subscribers []map[string]any
	tags        []map[string]any
}

// clock is the fixed time a test's source reads.
var clock = time.Date(2024, 1, 2, 3, 4, 5, 0, time.UTC)

// fakeKit answers the four lists and the custom field definitions from
// fixtures, pages by cursor, filters subscribers by updated_after and
// status, and refuses a wrong key.
func fakeKit(t *testing.T) (*httptest.Server, *[]string, *fixtures) {
	t.Helper()
	var calls []string
	data := &fixtures{
		subscribers: []map[string]any{
			{"id": 1.0, "first_name": "Ann", "email_address": "Ann@Northwind.example", "state": "active", "created_at": "2023-11-14T22:13:20Z", "fields": map[string]any{"Last name": "Lee", "Company": "Northwind Traders", "Email": "shadow@northwind.example"}, "updated_at": "2023-11-15T10:00:00Z"},
			{"id": 2.0, "first_name": nil, "email_address": "bo@fabrikam.example", "state": "cancelled", "created_at": "2023-11-13T09:00:00Z", "fields": map[string]any{"Last name": "Fabrik", "Company": nil}, "updated_at": "2023-11-13T09:00:00Z"},
			{"id": 3.0, "first_name": "Cy", "email_address": "cy@contoso.example", "state": "bounced", "created_at": "2023-11-12T09:00:00Z", "fields": map[string]any{}, "updated_at": "2023-11-12T09:00:00Z"},
		},
		tags: []map[string]any{
			{"id": 10.0, "name": "vip", "created_at": "2023-01-01T00:00:00Z"},
			{"id": 11.0, "name": "finance", "created_at": "2023-01-02T00:00:00Z"},
		},
	}
	forms := []map[string]any{
		{"id": 20.0, "name": "Newsletter signup", "type": "embed", "format": "inline", "uid": "abc123", "embed_url": "https://kit.example/abc123", "archived": false, "created_at": "2023-01-01T00:00:00Z"},
	}
	sequences := []map[string]any{
		{"id": 30.0, "name": "Welcome", "hold": false, "repeat": true, "created_at": "2023-01-01T00:00:00Z"},
	}
	customFields := []map[string]any{
		{"id": 1.0, "name": "ck_field_1_last_name", "key": "last_name", "label": "Last name"},
		{"id": 2.0, "name": "ck_field_2_company", "key": "company", "label": "Company"},
		{"id": 3.0, "name": "ck_field_3_email", "key": "email", "label": "Email"},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		switch r.Header.Get("X-Kit-Api-Key") {
		case "key-good":
		case "key-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"errors":["Rate limit exceeded"]}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"errors":["The API key is invalid"]}`))
			return
		}
		query := r.URL.Query()
		var rows []map[string]any
		key := ""
		switch r.URL.Path {
		case "/v4/subscribers":
			key = "subscribers"
			for _, row := range data.subscribers {
				if status := query.Get("status"); status != "all" && status != "" && row["state"] != status {
					continue
				}
				if status := query.Get("status"); status == "" && row["state"] != "active" {
					continue
				}
				if since := query.Get("updated_after"); since != "" && row["updated_at"].(string) <= since {
					continue
				}
				rows = append(rows, row)
			}
		case "/v4/tags":
			key, rows = "tags", data.tags
		case "/v4/forms":
			key, rows = "forms", forms
		case "/v4/sequences":
			key, rows = "sequences", sequences
		case "/v4/custom_fields":
			key, rows = "custom_fields", customFields
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"errors":["Not found"]}`))
			return
		}
		page, pagination := paginate(rows, query)
		_ = json.NewEncoder(w).Encode(map[string]any{key: page, "pagination": pagination})
	}))
	t.Cleanup(server.Close)
	return server, &calls, data
}

// paginate cuts one page by per_page from the after cursor, which the fake
// writes as the start index.
func paginate(rows []map[string]any, query url.Values) ([]map[string]any, map[string]any) {
	perPage, _ := strconv.Atoi(query.Get("per_page"))
	if perPage <= 0 {
		perPage = 500
	}
	start, _ := strconv.Atoi(query.Get("after"))
	if start > len(rows) {
		start = len(rows)
	}
	end := start + perPage
	if end > len(rows) {
		end = len(rows)
	}
	if rows == nil {
		rows = []map[string]any{}
	}
	return rows[start:end], map[string]any{
		"has_previous_page": start > 0,
		"has_next_page":     end < len(rows),
		"start_cursor":      strconv.Itoa(start),
		"end_cursor":        strconv.Itoa(end),
		"per_page":          perPage,
	}
}

func fixed(key, baseURL string) *Source {
	source := New(key, baseURL)
	source.now = func() time.Time { return clock }
	return source
}

func TestObjectsProveTheKeyAndListFour(t *testing.T) {
	server, calls, _ := fakeKit(t)
	source := fixed("key-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "subscribers" || objects[1].Name != "tags" || objects[2].Name != "forms" || objects[3].Label != "Sequences" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.Contains((*calls)[0], "/v4/tags?per_page=1") {
		t.Fatalf("the key was proved with one small call: %v", *calls)
	}
	if live := New("x", ""); live.base != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.base)
	}
	_, err = fixed("key-bad", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the API key") {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = fixed("key-slow", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("broadcasts")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"errors":["Your plan does not include API access"]}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "Your plan") {
		t.Fatalf("a 403 names the error, got %v", err)
	}
}

func TestDescribeFlattensSubscribersWithCustomFields(t *testing.T) {
	server, calls, _ := fakeKit(t)
	source := fixed("key-good", server.URL)
	description, err := source.Describe("subscribers")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Subscribers" || description.Source != "kit" || description.Hash != "2024-01-02T03:04:05Z" {
		t.Fatalf("description: %+v", description)
	}
	if len(description.Fields) != len(objects[0].fields)+2 {
		t.Fatalf("the core columns and the two custom fields the core leaves: %d", len(description.Fields))
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["created"].Guess != "date-time" || byName["company"].Guess != "string" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["email"].Samples[0] != "Ann@Northwind.example" || byName["state"].Samples[1] != "cancelled" {
		t.Fatalf("emails and states: %+v %+v", byName["email_domain"], byName["state"])
	}
	if byName["name"].Samples[0] != "Ann" || byName["name"].Samples[1] != "bo@fabrikam.example" || byName["name"].Samples[2] != "Cy" {
		t.Fatalf("names fall back to the email: %v", byName["name"].Samples)
	}
	if byName["last_name"].Samples[0] != "Lee" || byName["last_name"].Filled != 2 || byName["company"].Samples[0] != "Northwind Traders" || byName["company"].Filled != 1 {
		t.Fatalf("the custom field is a column named as the account named it: %+v %+v", byName["last_name"], byName["company"])
	}
	if byName["email"].Samples[0] == "shadow@northwind.example" || byName["email"].Filled != 3 {
		t.Fatal("a custom field named as a core column is no column")
	}
	if byName["created"].Samples[0] != "2023-11-14T22:13:20Z" {
		t.Fatalf("times: %v", byName["created"].Samples)
	}
	definitions, all := 0, false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v4/custom_fields") {
			definitions++
		}
		if strings.HasPrefix(call, "/v4/subscribers?") && strings.Contains(call, "status=all") && strings.Contains(call, "per_page=100") {
			all = true
		}
	}
	if definitions != 1 || !all {
		t.Fatalf("the custom field definitions read once and the list asks for every state: %v", *calls)
	}

	forms, err := source.Describe("forms")
	if err != nil {
		t.Fatal(err)
	}
	if forms.Rows != 1 || len(forms.Hash) != 64 || forms.Label != "Forms" {
		t.Fatalf("forms: %+v", forms)
	}
	byName = map[string]FieldDescription{}
	for _, field := range forms.Fields {
		byName[field.Name] = field
	}
	if byName["archived"].Samples[0] != "no" || byName["embed_url"].Samples[0] != "https://kit.example/abc123" || byName["type"].Samples[0] != "embed" {
		t.Fatalf("form fields: %v %v", byName["archived"].Samples, byName["embed_url"].Samples)
	}
	if _, custom := byName["company"]; custom {
		t.Fatal("a form carries no custom fields")
	}
}

func TestPageWalksCursors(t *testing.T) {
	server, calls, _ := fakeKit(t)
	source := fixed("key-good", server.URL)
	first, err := source.Page("subscribers", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2024-01-02T03:04:05Z" || first.Rows[0]["id"] != "1" || first.Rows[0]["last_name"] != "Lee" {
		t.Fatalf("next cursor: %+v %v", first.Next, first.Rows)
	}
	later := fixed("key-good", server.URL)
	later.now = func() time.Time { return clock.Add(time.Hour) }
	second, err := later.Page("subscribers", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" || second.Rows[0]["name"] != "Cy" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2024-01-02T03:04:05Z" {
		t.Fatalf("a later page carries the first page's mark: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v4/subscribers?") && strings.Contains(call, "after=2") && strings.Contains(call, "per_page=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not pass the end cursor: %v", *calls)
	}

	tags, err := source.Page("tags", nil, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(tags.Rows) != 1 || tags.Next == nil || tags.Next.Token != "1" || len(tags.Hash) != 64 || tags.Rows[0]["name"] != "vip" || tags.Rows[0]["id"] != "10" {
		t.Fatalf("tags first page: %+v", tags)
	}
	rest, err := source.Page("tags", tags.Next, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(rest.Rows) != 1 || rest.Offset != 1 || rest.Next != nil || rest.Rows[0]["name"] != "finance" || rest.Hash != tags.Hash {
		t.Fatalf("tags second page carries the mark: %+v", rest)
	}
	sequences, err := source.Page("sequences", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if sequences.Rows[0]["hold"] != "no" || sequences.Rows[0]["repeat"] != "yes" || sequences.Rows[0]["created"] != "2023-01-01T00:00:00Z" {
		t.Fatalf("sequence row: %v", sequences.Rows[0])
	}
}

func TestDeltaFiltersAfterTheMark(t *testing.T) {
	server, calls, data := fakeKit(t)
	source := fixed("key-good", server.URL)
	delta, err := source.Delta("subscribers", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("subscribers", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2024-01-02T03:04:05Z" {
		t.Fatalf("a change moves the mark to the time asked: %+v", delta)
	}
	delta, err = source.Delta("subscribers", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2024-01-02T03:04:05Z" {
		t.Fatalf("a first delta reads the clock: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/v4/subscribers?") && strings.Contains(call, "updated_after=") && strings.Contains(call, "per_page=1") && strings.Contains(call, "status=all") {
			filtered++
		}
	}
	if filtered != 2 {
		t.Fatalf("each marked delta is one filtered list: %d in %v", filtered, *calls)
	}

	first, err := source.Page("tags", nil, PageLimit)
	if err != nil {
		t.Fatal(err)
	}
	delta, err = source.Delta("tags", &Cursor{Offset: 2, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("an unchanged tags page keeps its hash: %+v", delta)
	}
	data.tags = append(data.tags, map[string]any{"id": 12.0, "name": "churned", "created_at": "2024-01-01T00:00:00Z"})
	delta, err = source.Delta("tags", &Cursor{Offset: 2, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash == first.Hash || len(delta.Hash) != 64 {
		t.Fatalf("a new tag changes the hash: %+v", delta)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.000Z") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if columnName("Last name") != "last_name" || columnName(" Setup fee (USD) ") != "setup_fee_usd" {
		t.Fatal("column names did not fold")
	}
	if kitMessage([]byte(`{"errors":["one","two"]}`)) != "one, two" || kitMessage([]byte(`{"error":"Unauthorized"}`)) != "Unauthorized" || kitMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a message reads its errors, then its error, then the body")
	}
	if hashRows(nil) != hashRows([]Row{}) || hashRows([]Row{{"a": "1"}}) == hashRows([]Row{{"a": "2"}}) {
		t.Fatal("the hash did not follow the rows")
	}
	if fixed("x", "").mark() != "2024-01-02T03:04:05Z" {
		t.Fatal("the mark is the clock as RFC 3339 in UTC")
	}
}
