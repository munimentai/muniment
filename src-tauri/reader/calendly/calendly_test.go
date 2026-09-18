package calendly

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"testing"
)

// fixtures is the fake account: the lists a test may grow between calls.
type fixtures struct {
	eventTypes []map[string]any
	events     []map[string]any
	invitees   map[string][]map[string]any
}

const userURI = "https://api.calendly.com/users/USER1"

// fakeCalendly answers the current user, the three lists and each event's
// invitees from fixtures, pages by count and page token, scopes the lists
// to the user, and refuses a wrong token.
func fakeCalendly(t *testing.T) (*httptest.Server, *[]string, *fixtures) {
	t.Helper()
	var calls []string
	data := &fixtures{
		eventTypes: []map[string]any{
			{"uri": "https://api.calendly.com/event_types/ET1", "name": "Discovery call", "slug": "discovery", "active": true, "duration": 30.0, "kind": "solo", "pooling_type": nil, "type": "StandardEventType", "scheduling_url": "https://calendly.com/ann/discovery", "description_plain": "First call", "secret": false, "booking_method": "instant", "color": "#ff0000",
				"profile": map[string]any{"type": "User", "name": "Ann Lee", "owner": userURI}, "created_at": "2023-11-14T22:13:20.123456Z", "updated_at": "2023-11-15T10:00:00.000000Z"},
			{"uri": "https://api.calendly.com/event_types/ET2", "name": "Renewal review", "slug": "renewal", "active": false, "duration": 60.0, "kind": "group", "type": "StandardEventType", "secret": true, "profile": map[string]any{"type": "Team", "name": "Sales", "owner": "https://api.calendly.com/organizations/ORG1"}, "created_at": "2023-11-13T09:00:00Z", "updated_at": "2023-11-13T09:00:00Z"},
			{"uri": "https://api.calendly.com/event_types/ET3", "name": "Support", "slug": "support", "active": true, "duration": 15.0, "kind": "solo", "created_at": "2023-11-12T09:00:00Z", "updated_at": "2023-11-12T09:00:00Z"},
		},
		events: []map[string]any{
			{"uri": "https://api.calendly.com/scheduled_events/E1", "name": "Discovery call", "status": "active", "start_time": "2023-11-20T15:00:00.000000Z", "end_time": "2023-11-20T15:30:00.000000Z", "event_type": "https://api.calendly.com/event_types/ET1",
				"location": map[string]any{"type": "zoom", "join_url": "https://zoom.example/1"}, "invitees_counter": map[string]any{"total": 2.0, "active": 2.0, "limit": 2.0},
				"event_memberships": []any{map[string]any{"user": userURI, "user_email": "ann@northwind.example", "user_name": "Ann Lee"}}, "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T10:00:00Z"},
			{"uri": "https://api.calendly.com/scheduled_events/E2", "name": "Renewal review", "status": "canceled", "start_time": "2023-11-21T15:00:00Z", "end_time": "2023-11-21T16:00:00Z", "event_type": "https://api.calendly.com/event_types/ET2",
				"location": map[string]any{"type": "physical", "location": "Seattle office"}, "invitees_counter": map[string]any{"total": 0.0, "active": 0.0, "limit": 5.0},
				"cancellation": map[string]any{"canceled_by": "Ann Lee", "reason": "moved", "canceler_type": "host"}, "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T11:00:00Z"},
			{"uri": "https://api.calendly.com/scheduled_events/E3", "name": "Support", "status": "active", "start_time": "2023-11-22T15:00:00Z", "end_time": "2023-11-22T15:15:00Z", "event_type": "https://api.calendly.com/event_types/ET3",
				"invitees_counter": map[string]any{"total": 2.0, "active": 1.0, "limit": 2.0}, "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T12:00:00Z"},
		},
		invitees: map[string][]map[string]any{
			"E1": {
				{"uri": "https://api.calendly.com/scheduled_events/E1/invitees/I1", "email": "Bo@Fabrikam.example", "name": "Bo Fabrik", "first_name": "Bo", "last_name": "Fabrik", "status": "active", "timezone": "America/Los_Angeles", "event": "https://api.calendly.com/scheduled_events/E1", "rescheduled": false, "no_show": nil, "scheduling_method": nil,
					"tracking": map[string]any{"utm_source": "newsletter", "utm_campaign": "q4"}, "questions_and_answers": []any{map[string]any{"question": "Company", "answer": "Fabrikam", "position": 0.0}, map[string]any{"question": "Size", "answer": "50", "position": 1.0}},
					"created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T10:00:00Z"},
				{"uri": "https://api.calendly.com/scheduled_events/E1/invitees/I2", "email": "cy@contoso.example", "name": "Cy Con", "status": "active", "event": "https://api.calendly.com/scheduled_events/E1", "rescheduled": true, "no_show": map[string]any{"uri": "https://api.calendly.com/invitee_no_shows/N1", "created_at": "2023-11-20T15:20:00Z"}, "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T10:00:00Z"},
			},
			"E3": {
				{"uri": "https://api.calendly.com/scheduled_events/E3/invitees/I3", "email": "di@northwind.example", "name": "Di North", "status": "canceled", "event": "https://api.calendly.com/scheduled_events/E3", "cancellation": map[string]any{"canceled_by": "Di North", "reason": "busy", "canceler_type": "invitee"}, "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T12:00:00Z"},
				{"uri": "https://api.calendly.com/scheduled_events/E3/invitees/I4", "email": "ed@northwind.example", "name": "Ed North", "status": "active", "event": "https://api.calendly.com/scheduled_events/E3", "created_at": "2023-11-14T22:13:20Z", "updated_at": "2023-11-15T12:00:00Z"},
			},
		},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		switch r.Header.Get("Authorization") {
		case "Bearer token-good":
		case "Bearer token-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"title":"Too Many Requests","message":"Rate limit exceeded"}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"title":"Unauthenticated","message":"The access token is invalid"}`))
			return
		}
		query := r.URL.Query()
		if r.URL.Path == "/users/me" {
			_ = json.NewEncoder(w).Encode(map[string]any{"resource": map[string]any{"uri": userURI, "name": "Ann Lee", "email": "ann@northwind.example"}})
			return
		}
		var rows []map[string]any
		switch {
		case r.URL.Path == "/event_types" || r.URL.Path == "/scheduled_events":
			if query.Get("user") != userURI {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"title":"Invalid Argument","message":"The supplied parameters are invalid.","details":[{"parameter":"user","message":"is required"}]}`))
				return
			}
			rows = data.eventTypes
			if r.URL.Path == "/scheduled_events" {
				rows = data.events
			}
		case strings.HasPrefix(r.URL.Path, "/scheduled_events/") && strings.HasSuffix(r.URL.Path, "/invitees"):
			id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/scheduled_events/"), "/invitees")
			listed, ok := data.invitees[id]
			if !ok {
				w.WriteHeader(http.StatusNotFound)
				_, _ = w.Write([]byte(`{"title":"Resource Not Found","message":"The requested resource was not found"}`))
				return
			}
			rows = listed
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"title":"Resource Not Found","message":"The requested resource was not found"}`))
			return
		}
		page, next := paginate(rows, query)
		pagination := map[string]any{"count": len(page), "next_page": nil, "next_page_token": nil, "previous_page": nil, "previous_page_token": nil}
		if next != "" {
			pagination["next_page_token"] = next
			pagination["next_page"] = "https://api.calendly.com" + r.URL.Path + "?page_token=" + next
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"collection": page, "pagination": pagination})
	}))
	t.Cleanup(server.Close)
	return server, &calls, data
}

// paginate cuts one page by count from the page token, which the fake
// writes as the start index.
func paginate(rows []map[string]any, query url.Values) ([]map[string]any, string) {
	count, _ := strconv.Atoi(query.Get("count"))
	if count <= 0 {
		count = 20
	}
	start, _ := strconv.Atoi(query.Get("page_token"))
	if start > len(rows) {
		start = len(rows)
	}
	end := start + count
	if end > len(rows) {
		end = len(rows)
	}
	next := ""
	if end < len(rows) {
		next = strconv.Itoa(end)
	}
	return rows[start:end], next
}

func TestObjectsProveTheTokenAndListThree(t *testing.T) {
	server, calls, _ := fakeCalendly(t)
	source := New("token-good", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "event_types" || objects[1].Name != "scheduled_events" || objects[2].Label != "Invitees" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "/users/me?") {
		t.Fatalf("the token was proved with one read of the current user: %v", *calls)
	}
	if live := New("x", ""); live.base != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.base)
	}
	_, err = New("token-bad", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the personal access token") {
		t.Fatalf("a refused token must read as not_connected, got %v", err)
	}
	_, err = New("token-slow", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("routing_forms")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"title":"Permission Denied","message":"You are not allowed to access this resource"}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "not allowed") {
		t.Fatalf("a 403 names the message, got %v", err)
	}
}

func TestDescribeFlattensScopedLists(t *testing.T) {
	server, calls, _ := fakeCalendly(t)
	source := New("token-good", server.URL)
	description, err := source.Describe("scheduled_events")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Scheduled events" || description.Source != "calendly" || len(description.Hash) != 64 {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["event_type_id"].Guess != "id" || byName["start_time"].Guess != "date-time" || byName["invitees_total"].Guess != "number" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["id"].Samples[0] != "E1" || byName["event_type_id"].Samples[0] != "ET1" || byName["hosts"].Samples[0] != "Ann Lee" || byName["host_ids"].Samples[0] != "USER1" {
		t.Fatalf("ids read off the uris: %+v %+v", byName["id"], byName["host_ids"])
	}
	if byName["location_type"].Samples[1] != "physical" || byName["join_url"].Samples[0] != "https://zoom.example/1" || byName["invitees_total"].Samples[0] != "2" || byName["cancel_reason"].Samples[0] != "moved" {
		t.Fatalf("locations, counters and cancellations: %v %v", byName["location_type"].Samples, byName["cancel_reason"].Samples)
	}
	if byName["start_time"].Samples[0] != "2023-11-20T15:00:00Z" || byName["status"].Filled != 3 {
		t.Fatalf("times and statuses: %+v %+v", byName["start_time"], byName["status"])
	}
	scoped := false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/scheduled_events?") && strings.Contains(call, "user="+url.QueryEscape(userURI)) && strings.Contains(call, "count=100") {
			scoped = true
		}
	}
	if !scoped {
		t.Fatalf("the list is scoped to the current user: %v", *calls)
	}

	types, err := source.Describe("event_types")
	if err != nil {
		t.Fatal(err)
	}
	byName = map[string]FieldDescription{}
	for _, field := range types.Fields {
		byName[field.Name] = field
	}
	if byName["duration"].Samples[0] != "30" || byName["active"].Samples[0] != "yes" || byName["active"].Samples[1] != "no" || byName["owner_id"].Samples[0] != "USER1" || byName["owner_id"].Samples[1] != "ORG1" {
		t.Fatalf("event type row: %v %v %v", byName["duration"].Samples, byName["active"].Samples, byName["owner_id"].Samples)
	}
	if byName["created"].Samples[0] != "2023-11-14T22:13:20Z" || byName["pooling_type"].Filled != 0 {
		t.Fatalf("times and nulls: %+v %+v", byName["created"], byName["pooling_type"])
	}
	users := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/users/me") {
			users++
		}
	}
	if users != 1 {
		t.Fatalf("the current user reads once per source: %v", *calls)
	}
}

func TestPageWalksInviteesPerEvent(t *testing.T) {
	server, calls, _ := fakeCalendly(t)
	source := New("token-good", server.URL)
	first, err := source.Page("invitees", nil, 3)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 3 || first.Offset != 0 || first.Total != 3 || first.Counted || first.Next == nil || len(first.Hash) != 64 {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Offset != 3 || first.Next.Hash != first.Hash || !strings.Contains(first.Next.Token, `"at":2`) || !strings.Contains(first.Next.Token, `"invitees":"1"`) {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["id"] != "I1" || row["email"] != "Bo@Fabrikam.example" || row["email_domain"] != "fabrikam.example" || row["event_id"] != "E1" || row["event_name"] != "Discovery call" || row["event_type_id"] != "ET1" || row["event_start_time"] != "2023-11-20T15:00:00Z" {
		t.Fatalf("invitee row: %v", row)
	}
	if row["answers"] != "Company: Fabrikam | Size: 50" || row["utm_source"] != "newsletter" || row["no_show"] != "no" || row["rescheduled"] != "no" {
		t.Fatalf("answers, tracking and flags: %v", row)
	}
	if first.Rows[1]["no_show"] != "yes" || first.Rows[1]["rescheduled"] != "yes" || first.Rows[2]["id"] != "I3" || first.Rows[2]["cancel_reason"] != "busy" || first.Rows[2]["event_name"] != "Support" {
		t.Fatalf("later rows: %v %v", first.Rows[1], first.Rows[2])
	}
	second, err := source.Page("invitees", first.Next, 3)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 3 || second.Total != 4 || second.Next != nil || second.Rows[0]["id"] != "I4" || second.Hash != first.Hash {
		t.Fatalf("second page: %+v", second)
	}
	for _, call := range *calls {
		if strings.HasPrefix(call, "/scheduled_events/E2/") {
			t.Fatalf("an event that counts no invitees costs no call: %v", *calls)
		}
	}
	resumed := false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/scheduled_events/E3/invitees?") && strings.Contains(call, "page_token=1") {
			resumed = true
		}
	}
	if !resumed {
		t.Fatalf("the second page resumed inside the event's invitees: %v", *calls)
	}

	types, err := source.Page("event_types", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(types.Rows) != 2 || types.Next == nil || types.Next.Token != "2" || types.Rows[0]["name"] != "Discovery call" {
		t.Fatalf("event types first page: %+v", types)
	}
	rest, err := source.Page("event_types", types.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(rest.Rows) != 1 || rest.Offset != 2 || rest.Total != 3 || rest.Next != nil || rest.Rows[0]["id"] != "ET3" || rest.Hash != types.Hash {
		t.Fatalf("event types second page: %+v", rest)
	}
	found := false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/event_types?") && strings.Contains(call, "page_token=2") && strings.Contains(call, "count=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not pass the page token: %v", *calls)
	}
}

func TestDeltaHashesTheFirstPage(t *testing.T) {
	server, calls, data := fakeCalendly(t)
	source := New("token-good", server.URL)
	first, err := source.Page("event_types", nil, PageLimit)
	if err != nil {
		t.Fatal(err)
	}
	delta, err := source.Delta("event_types", &Cursor{Offset: 3, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("event_types", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != first.Hash {
		t.Fatalf("a first delta reads the first page's hash: %+v", delta)
	}
	data.eventTypes[0]["name"] = "Discovery call (30 min)"
	delta, err = source.Delta("event_types", &Cursor{Offset: 3, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash == first.Hash || len(delta.Hash) != 64 {
		t.Fatalf("a renamed event type changes the mark: %+v", delta)
	}
	invitees, err := source.Delta("invitees", nil)
	if err != nil {
		t.Fatal(err)
	}
	if invitees.State != "changed" || len(invitees.Hash) != 64 || invitees.Hash == delta.Hash {
		t.Fatalf("the invitees delta hashes the walk's first page: %+v", invitees)
	}
	listed := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/event_types?") && strings.Contains(call, "count=100") {
			listed++
		}
	}
	if listed != 4 {
		t.Fatalf("each delta is one first page: %d in %v", listed, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.123456Z") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if uuid("https://api.calendly.com/users/ABC") != "ABC" || uuid("ABC") != "ABC" || uuid("") != "" {
		t.Fatal("ids did not read off the uris")
	}
	if hashRows(nil) != hashRows([]Row{}) || hashRows([]Row{{"a": "1"}}) == hashRows([]Row{{"a": "2"}}) {
		t.Fatal("the hash did not follow the rows")
	}
	if calendlyMessage([]byte(`{"title":"Invalid Argument"}`)) != "Invalid Argument" || calendlyMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a message reads its title, then the body")
	}
	if (walk{Events: "x", At: 2}).encode() != `{"events":"x","at":2}` {
		t.Fatal("the walk token leaves empty parts out")
	}
}
