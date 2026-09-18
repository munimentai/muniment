package notion

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

const dealsID = "d1000000-0000-0000-0000-000000000001"
const peopleID = "d1000000-0000-0000-0000-000000000002"

func richText(text string) []any {
	return []any{map[string]any{"type": "text", "plain_text": text}}
}

func page(id, edited, name string, amount float64, stage string, owners []any, companies []any) map[string]any {
	return map[string]any{
		"object": "page", "id": id, "url": "https://www.notion.so/" + id, "archived": false,
		"created_time": "2023-11-01T09:00:00.000Z", "last_edited_time": edited,
		"created_by": map[string]any{"object": "user", "id": "u1"}, "last_edited_by": map[string]any{"object": "user", "id": "u2"},
		"parent": map[string]any{"type": "database_id", "database_id": dealsID},
		"properties": map[string]any{
			"Name":       map[string]any{"type": "title", "title": richText(name)},
			"Amount":     map[string]any{"type": "number", "number": amount},
			"Stage":      map[string]any{"type": "select", "select": map[string]any{"name": stage}},
			"Tags":       map[string]any{"type": "multi_select", "multi_select": []any{map[string]any{"name": "b"}, map[string]any{"name": "a"}}},
			"Owner":      map[string]any{"type": "people", "people": owners},
			"Company":    map[string]any{"type": "relation", "relation": companies},
			"Close date": map[string]any{"type": "date", "date": map[string]any{"start": "2023-12-16", "end": nil}},
			"Won":        map[string]any{"type": "checkbox", "checkbox": stage == "Won"},
			"Notes":      map[string]any{"type": "rich_text", "rich_text": richText("call " + name)},
			"Ref":        map[string]any{"type": "unique_id", "unique_id": map[string]any{"prefix": "DL", "number": 7.0}},
			"Total":      map[string]any{"type": "formula", "formula": map[string]any{"type": "number", "number": amount * 2}},
			"Go":         map[string]any{"type": "button", "button": map[string]any{}},
		},
	}
}

// fakeNotion answers the search and the database query from fixtures,
// pages by start cursor, filters by last edit, and refuses a wrong token.
func fakeNotion(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	schema := map[string]any{
		"Name": map[string]any{"type": "title"}, "Amount": map[string]any{"type": "number"}, "Stage": map[string]any{"type": "select"},
		"Tags": map[string]any{"type": "multi_select"}, "Owner": map[string]any{"type": "people"}, "Company": map[string]any{"type": "relation", "relation": map[string]any{"database_id": peopleID}},
		"Close date": map[string]any{"type": "date"}, "Won": map[string]any{"type": "checkbox"}, "Notes": map[string]any{"type": "rich_text"},
		"Ref": map[string]any{"type": "unique_id"}, "Total": map[string]any{"type": "formula", "formula": map[string]any{"expression": "prop(\"Amount\") * 2"}},
		"Go": map[string]any{"type": "button"},
	}
	databases := []map[string]any{
		{"object": "database", "id": dealsID, "title": richText("Deals"), "properties": schema},
		{"object": "database", "id": peopleID, "title": []any{}, "properties": map[string]any{"Name": map[string]any{"type": "title"}}},
	}
	pages := []map[string]any{
		page("p1", "2023-11-15T10:00:00.000Z", "Northwind renewal", 96000, "Negotiation", []any{map[string]any{"id": "u1", "name": "Mikey"}, map[string]any{"id": "u3", "name": "Ann"}}, []any{map[string]any{"id": "c1"}}),
		page("p2", "2023-11-13T09:00:00.000Z", "Contoso pilot", 1200, "Won", []any{}, []any{}),
		page("p3", "2023-11-12T09:00:00.000Z", "Fabrikam", 0, "", nil, nil),
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path)
		if r.Header.Get("Notion-Version") == "" {
			t.Errorf("every call names the API version")
		}
		switch r.Header.Get("Authorization") {
		case "Bearer tok-good":
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"object":"error","status":401,"code":"unauthorized","message":"API token is invalid."}`))
			return
		}
		raw, _ := io.ReadAll(r.Body)
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		switch {
		case r.URL.Path == "/v1/search":
			if body["start_cursor"] == nil {
				_ = json.NewEncoder(w).Encode(map[string]any{"results": databases[:1], "has_more": true, "next_cursor": "s2"})
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"results": databases[1:], "has_more": false, "next_cursor": nil})
		case r.URL.Path == "/v1/databases/"+dealsID && r.Method == http.MethodGet:
			_ = json.NewEncoder(w).Encode(databases[0])
		case r.URL.Path == "/v1/databases/"+dealsID+"/query":
			rows := pages
			if filter, ok := body["filter"].(map[string]any); ok {
				after := filter["last_edited_time"].(map[string]any)["after"].(string)
				rows = []map[string]any{}
				for _, entry := range pages {
					if entry["last_edited_time"].(string) > after {
						rows = append(rows, entry)
					}
				}
			}
			size := int(body["page_size"].(float64))
			start := 0
			if cursor, ok := body["start_cursor"].(string); ok {
				start = int(cursor[0] - '0')
			}
			end := start + size
			if end > len(rows) {
				end = len(rows)
			}
			answer := map[string]any{"results": rows[start:end], "has_more": end < len(rows), "next_cursor": nil}
			if end < len(rows) {
				answer["next_cursor"] = string(rune('0' + end))
			}
			_ = json.NewEncoder(w).Encode(answer)
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"object":"error","status":404,"code":"object_not_found","message":"Could not find database."}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func TestObjectsWalkTheSearch(t *testing.T) {
	server, calls := fakeNotion(t)
	source := New("tok-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != dealsID || objects[0].Label != "Deals" || objects[1].Label != "Untitled" {
		t.Fatalf("objects: %+v", objects)
	}
	searches := 0
	for _, call := range *calls {
		if call == "POST /v1/search" {
			searches++
		}
	}
	if searches != 2 {
		t.Fatalf("the search walks both pages: %v", *calls)
	}
	_, err = New("tok-bad", server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the integration token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = source.Describe("missing")
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown database must read as unknown_object, got %v", err)
	}
}

func TestDescribeFlattensProperties(t *testing.T) {
	server, _ := fakeNotion(t)
	description, err := New("tok-good", server.URL).Describe(dealsID)
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Deals" || description.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["amount"].Guess != "number" || byName["won"].Guess != "boolean" || byName["close_date"].Guess != "date" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["name"].Samples[0] != "Northwind renewal" || byName["amount"].Samples[0] != "96000" || byName["stage"].Samples[0] != "Negotiation" {
		t.Fatalf("values: %+v %+v", byName["name"], byName["amount"])
	}
	if byName["tags"].Samples[0] != "a,b" || byName["owner"].Samples[0] != "Mikey,Ann" || byName["owner_ids"].Samples[0] != "u1,u3" || byName["company"].Samples[0] != "c1" {
		t.Fatalf("lists and ids: %+v %+v %+v", byName["tags"], byName["owner_ids"], byName["company"])
	}
	if byName["ref"].Samples[0] != "DL-7" || byName["total"].Samples[0] != "192000" || byName["notes"].Samples[0] != "call Northwind renewal" || byName["won"].Samples[0] != "no" {
		t.Fatalf("computed values: %+v %+v", byName["ref"], byName["total"])
	}
	if byName["parent"].Samples[0] != dealsID || byName["modified_by"].Samples[0] != "u2" || byName["created"].Samples[0] != "2023-11-01T09:00:00Z" {
		t.Fatalf("core columns: %+v %+v", byName["parent"], byName["created"])
	}
	if _, ok := byName["go"]; ok {
		t.Fatal("a button is not a column")
	}
}

func TestPageWalksTheCursor(t *testing.T) {
	server, _ := fakeNotion(t)
	source := New("tok-good", server.URL)
	first, err := source.Page(dealsID, nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page(dealsID, first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "p3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
}

func TestDeltaFiltersByLastEdit(t *testing.T) {
	server, _ := fakeNotion(t)
	source := New("tok-good", server.URL)
	delta, err := source.Delta(dealsID, &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta(dealsID, &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta(dealsID, nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
}

func TestValuesRead(t *testing.T) {
	if columnName("Close date") != "close_date" || columnName(" Amount (USD) ") != "amount_usd" {
		t.Fatal("column names did not fold")
	}
	if formatTime("2023-11-14T22:13:20.000Z") != "2023-11-14T22:13:20Z" || formatTime("2023-12-16") != "2023-12-16" || formatTime("") != "" {
		t.Fatal("times did not read")
	}
	if newestEdit(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" || newestEdit([]item{{"last_edited_time": "2022-01-01T00:00:00.000Z"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
}
