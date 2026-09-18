package airtable

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

const secret = `{"token":"pat-good","base_id":"appBASE0000000001"}`

// fakeAirtable answers the base schema and the record lists from fixtures,
// pages by offset, filters by the modified time formula, and refuses a
// wrong token.
func fakeAirtable(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	schema := map[string]any{"tables": []any{
		map[string]any{"id": "tblPEOPLE", "name": "People", "fields": []any{
			map[string]any{"id": "fldName", "name": "Name", "type": "singleLineText"},
			map[string]any{"id": "fldEmail", "name": "Email", "type": "email"},
			map[string]any{"id": "fldCompany", "name": "Company", "type": "multipleRecordLinks", "options": map[string]any{"linkedTableId": "tblORGS"}},
			map[string]any{"id": "fldOwner", "name": "Owner", "type": "singleCollaborator"},
			map[string]any{"id": "fldTags", "name": "Tags", "type": "multipleSelects"},
			map[string]any{"id": "fldScore", "name": "Lead Score", "type": "number", "options": map[string]any{"precision": 0}},
			map[string]any{"id": "fldActive", "name": "Active", "type": "checkbox"},
			map[string]any{"id": "fldSeen", "name": "Last seen", "type": "dateTime"},
			map[string]any{"id": "fldTotal", "name": "Total", "type": "formula", "options": map[string]any{"result": map[string]any{"type": "number"}}},
			map[string]any{"id": "fldFiles", "name": "Files", "type": "multipleAttachments"},
		}},
		map[string]any{"id": "tblORGS", "name": "Organizations", "fields": []any{
			map[string]any{"id": "fldOrgName", "name": "Name", "type": "singleLineText"},
		}},
	}}
	people := []map[string]any{
		{"id": "rec1", "createdTime": "2023-11-14T22:13:20.000Z", "fields": map[string]any{
			"fldName": "Ann Lee", "fldEmail": "ann@northwind.example", "fldCompany": []any{"recORG1", "recORG2"},
			"fldOwner": map[string]any{"id": "usr1", "email": "mikey@example.com", "name": "Mikey"}, "fldTags": []any{"gold", "early"},
			"fldScore": 42.0, "fldActive": true, "fldSeen": "2023-11-15T10:00:00.000Z", "fldTotal": 84.0,
			"fldFiles": []any{map[string]any{"id": "att1", "filename": "deck.pdf", "url": "https://x/deck.pdf"}}}},
		{"id": "rec2", "createdTime": "2023-11-13T09:00:00.000Z", "fields": map[string]any{"fldName": "Bo Fabrik", "fldActive": false}},
		{"id": "rec3", "createdTime": "2023-11-12T09:00:00.000Z", "fields": map[string]any{"fldName": "Cy Contoso"}},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		switch r.Header.Get("Authorization") {
		case "Bearer pat-good":
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":{"type":"AUTHENTICATION_REQUIRED","message":"Authentication required"}}`))
			return
		}
		switch r.URL.Path {
		case "/v0/meta/bases/appBASE0000000001/tables":
			_ = json.NewEncoder(w).Encode(schema)
			return
		case "/v0/appBASE0000000001/tblPEOPLE":
			if r.URL.Query().Get("returnFieldsByFieldId") != "true" {
				t.Errorf("records read by field id: %s", r.URL.RawQuery)
			}
			rows := people
			if formula := r.URL.Query().Get("filterByFormula"); formula != "" {
				if !strings.HasPrefix(formula, "IS_AFTER(LAST_MODIFIED_TIME(), DATETIME_PARSE('") {
					t.Errorf("the delta filter is the modified time formula: %s", formula)
				}
				rows = []map[string]any{}
				if strings.Contains(formula, "2023-11-13T00:00:00Z") {
					rows = people[:1]
				}
			}
			size, _ := strconv.Atoi(r.URL.Query().Get("pageSize"))
			if size <= 0 {
				size = 100
			}
			start, _ := strconv.Atoi(strings.TrimPrefix(r.URL.Query().Get("offset"), "itr/"))
			end := start + size
			if end > len(rows) {
				end = len(rows)
			}
			answer := map[string]any{"records": rows[start:end]}
			if end < len(rows) {
				answer["offset"] = "itr/" + strconv.Itoa(end)
			}
			_ = json.NewEncoder(w).Encode(answer)
			return
		}
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":"NOT_FOUND"}`))
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func frozen(source *Source) *Source {
	source.now = func() time.Time { return time.Date(2023, 11, 16, 12, 0, 0, 0, time.UTC) }
	return source
}

func TestObjectsReadTheSchema(t *testing.T) {
	server, calls := fakeAirtable(t)
	source := New(secret, server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "tblPEOPLE" || objects[0].Label != "People" || objects[1].Label != "Organizations" {
		t.Fatalf("objects: %+v", objects)
	}
	if !strings.HasPrefix((*calls)[0], "/v0/meta/bases/appBASE0000000001/tables") {
		t.Fatalf("the schema proves the token: %v", *calls)
	}
	_, err = New(`{"token":"pat-bad","base_id":"appBASE0000000001"}`, server.URL).Objects()
	failure, ok := err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the personal access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New(`{"token":"pat-good","base_id":"appMISSING"}`, server.URL).Objects()
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "no base appMISSING") {
		t.Fatalf("a missing base must read as not_connected, got %v", err)
	}
	_, err = source.Describe("tblNOPE")
	failure, ok = err.(*Failure)
	if !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown table must read as unknown_object, got %v", err)
	}
	if baseID("https://airtable.com/appBASE0000000001/tblX/viwY?blocks=hide") != "appBASE0000000001" || baseID(" appZ ") != "appZ" {
		t.Fatal("the base id did not read from a URL")
	}
}

func TestDescribeFlattensFields(t *testing.T) {
	server, _ := fakeAirtable(t)
	description, err := frozen(New(secret, server.URL)).Describe("tblPEOPLE")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "People" || description.Hash != "2023-11-16T12:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["lead_score"].Guess != "number" || byName["active"].Guess != "boolean" || byName["last_seen"].Guess != "date-time" || byName["total"].Guess != "number" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["company"].Samples[0] != "recORG1,recORG2" || byName["owner"].Samples[0] != "Mikey" || byName["owner_id"].Samples[0] != "usr1" {
		t.Fatalf("links and collaborators: %+v %+v %+v", byName["company"], byName["owner"], byName["owner_id"])
	}
	if byName["tags"].Samples[0] != "early,gold" || byName["files"].Samples[0] != "deck.pdf" || byName["active"].Samples[1] != "no" || byName["created"].Samples[0] != "2023-11-14T22:13:20Z" {
		t.Fatalf("values: %+v %+v %+v", byName["tags"], byName["files"], byName["active"])
	}
	if byName["name"].Filled != 3 || byName["email"].Filled != 1 {
		t.Fatalf("fill counts: %+v %+v", byName["name"], byName["email"])
	}
}

func TestPageWalksTheOffset(t *testing.T) {
	server, calls := fakeAirtable(t)
	source := frozen(New(secret, server.URL))
	first, err := source.Page("tblPEOPLE", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "itr/2" || first.Next.Offset != 2 || first.Hash != "2023-11-16T12:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("tblPEOPLE", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "rec3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-16T12:00:00Z" {
		t.Fatalf("the mark carries through the pages: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/tblPEOPLE?") && strings.Contains(call, "offset=itr%2F2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not start at the offset: %v", *calls)
	}
}

func TestDeltaFiltersByModifiedTime(t *testing.T) {
	server, _ := fakeAirtable(t)
	source := frozen(New(secret, server.URL))
	delta, err := source.Delta("tblPEOPLE", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("tblPEOPLE", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T12:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("tblPEOPLE", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T12:00:00Z" {
		t.Fatalf("a first delta marks the check: %+v", delta)
	}
}

func TestValuesRead(t *testing.T) {
	if columnName("Lead Score") != "lead_score" || columnName(" Total (USD) ") != "total_usd" {
		t.Fatal("column names did not fold")
	}
	if text, ids := fieldValue("multipleCollaborators", []any{map[string]any{"id": "u1", "name": "A"}, map[string]any{"id": "u2", "email": "b@x"}}); text != "A,b@x" || ids != "u1,u2" {
		t.Fatalf("collaborators did not read: %s %s", text, ids)
	}
	if text, _ := fieldValue("formula", map[string]any{"error": "#ERROR!"}); text != "" {
		t.Fatal("a formula error reads as empty")
	}
	if text, _ := fieldValue("multipleLookupValues", []any{"a", 2.0}); text != "a,2" {
		t.Fatalf("lookups join: %s", text)
	}
	if guessType("rollup", map[string]any{"result": map[string]any{"type": "date"}}) != "date" || guessType("percent", nil) != "number" {
		t.Fatal("types did not guess")
	}
	if airtableMessage([]byte(`{"error":"NOT_FOUND"}`)) != "NOT_FOUND" || airtableMessage([]byte(`{"error":{"type":"X","message":"why"}}`)) != "why" {
		t.Fatal("messages did not read")
	}
}
