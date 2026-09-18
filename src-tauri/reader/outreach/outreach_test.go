package outreach

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
)

// fakeOutreach answers the token endpoint and the four lists from
// fixtures as JSON:API, pages by cursor, filters by updatedAt, and
// refuses a wrong refresh token.
func fakeOutreach(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	resource := func(kind string, id float64, attributes map[string]any, relationships map[string]any) map[string]any {
		rels := map[string]any{}
		for key, value := range relationships {
			rels[key] = map[string]any{"data": value}
		}
		return map[string]any{"type": kind, "id": id, "attributes": attributes, "relationships": rels}
	}
	ref := func(kind string, id float64) map[string]any { return map[string]any{"type": kind, "id": id} }
	prospects := []map[string]any{
		resource("prospect", 1, map[string]any{"firstName": "Ann", "lastName": "Lee", "name": "Ann Lee", "emails": []any{"Ann@Northwind.example", "old@northwind.example"}, "workPhones": []any{}, "mobilePhones": []any{"+1 555 010 0000"},
			"title": "CFO", "company": "Northwind Traders", "tags": []any{"vip", "finance"}, "optedOut": false, "engagedAt": "2023-11-14T22:13:20.000Z", "createdAt": "2023-11-14T22:13:20.000Z", "updatedAt": "2023-11-15T10:00:00.250Z", "custom3": "gold", "custom1": nil},
			map[string]any{"account": ref("account", 10), "owner": ref("user", 7), "stage": ref("stage", 2)}),
		resource("prospect", 2, map[string]any{"firstName": "Bo", "lastName": "Fabrik", "emails": []any{"bo@fabrikam.example"}, "optedOut": true, "createdAt": "2023-11-13T09:00:00.000Z", "updatedAt": "2023-11-13T09:00:00.000Z", "custom12": "west"},
			map[string]any{"account": nil}),
		resource("prospect", 3, map[string]any{"emails": []any{}, "createdAt": "2023-11-12T09:00:00.000Z", "updatedAt": "2023-11-12T09:00:00.000Z"}, nil),
	}
	accounts := []map[string]any{
		resource("account", 10, map[string]any{"name": "Northwind Traders", "domain": "northwind.example", "websiteUrl": "https://northwind.example", "industry": "Retail", "companyType": "Private", "numberOfEmployees": 120.0, "locality": "Seattle", "tags": []any{}, "createdAt": "2023-01-01T00:00:00.000Z", "updatedAt": "2023-11-15T11:00:00.000Z", "custom2": "tier-1"},
			map[string]any{"owner": ref("user", 7)}),
	}
	sequences := []map[string]any{
		resource("sequence", 20, map[string]any{"name": "Renewal outreach", "enabled": true, "sequenceType": "interval", "shareType": "shared", "sequenceStepCount": 5.0, "deliverCount": 40.0, "openCount": 22.0, "replyCount": 3.0, "bounceCount": 1.0, "createdAt": "2023-01-01T00:00:00.000Z", "updatedAt": "2023-11-15T12:00:00.000Z"},
			map[string]any{"owner": ref("user", 7)}),
	}
	mailboxes := []map[string]any{
		resource("mailbox", 30, map[string]any{"email": "mikey@muniment.example", "userName": "mikey", "provider": "gmail", "sendDisabled": false, "syncDisabled": false, "createdAt": "2023-01-01T00:00:00.000Z", "updatedAt": "2023-11-15T13:00:00.000Z"},
			map[string]any{"user": ref("user", 7)}),
	}
	data := map[string][]map[string]any{"/api/v2/prospects": prospects, "/api/v2/accounts": accounts, "/api/v2/sequences": sequences, "/api/v2/mailboxes": mailboxes}
	updated := func(row map[string]any) string { return row["attributes"].(map[string]any)["updatedAt"].(string) }
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/oauth/token" {
			_ = r.ParseForm()
			if r.Form.Get("grant_type") != "refresh_token" || r.Form.Get("client_id") != "cid" || r.Form.Get("client_secret") != "csecret" || r.Form.Get("refresh_token") != "rt-good" {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"invalid_grant","error_description":"The provided authorization grant is invalid"}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "at-good", "refresh_token": "rt-next", "token_type": "bearer", "expires_in": 7200})
			return
		}
		switch r.Header.Get("Authorization") {
		case "Bearer at-good":
		case "Bearer at-slow":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"errors":[{"id":"rateLimitExceeded","title":"Rate Limit Exceeded"}]}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"errors":[{"id":"unauthorizedRequest","title":"Unauthorized Request","detail":"The access token is invalid"}]}`))
			return
		}
		rows, ok := data[r.URL.Path]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"errors":[{"id":"notFound","title":"Not Found"}]}`))
			return
		}
		query := r.URL.Query()
		if filter := query.Get("filter[updatedAt]"); filter != "" {
			since := strings.TrimSuffix(filter, "..inf")
			kept := []map[string]any{}
			for _, row := range rows {
				if updated(row) >= since {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if query.Get("sort") == "-updatedAt" {
			sorted := append([]map[string]any(nil), rows...)
			for i := 0; i < len(sorted); i++ {
				for j := i + 1; j < len(sorted); j++ {
					if updated(sorted[j]) > updated(sorted[i]) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		size, _ := strconv.Atoi(query.Get("page[size]"))
		if size <= 0 {
			size = 100
		}
		start, _ := strconv.Atoi(query.Get("page[after]"))
		if start > len(rows) {
			start = len(rows)
		}
		end := start + size
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{"data": rows[start:end], "links": map[string]any{}}
		if end < len(rows) {
			answer["links"] = map[string]any{"next": server(r) + r.URL.Path + "?page%5Bafter%5D=" + strconv.Itoa(end) + "&page%5Bsize%5D=" + strconv.Itoa(size) + "&sort=id"}
		}
		w.Header().Set("Content-Type", "application/vnd.api+json")
		_ = json.NewEncoder(w).Encode(answer)
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func server(r *http.Request) string { return "http://" + r.Host }

func packed(refreshToken string) string {
	body, _ := json.Marshal(map[string]string{"client_id": "cid", "client_secret": "csecret", "refresh_token": refreshToken})
	return string(body)
}

func TestObjectsSignInOnceAndListFour(t *testing.T) {
	fake, calls := fakeOutreach(t)
	source := New(packed("rt-good"), fake.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "prospects" || objects[3].Label != "Mailboxes" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "POST /oauth/token") {
		t.Fatalf("objects is the one sign-in call: %v", *calls)
	}
	if live := New(packed("x"), ""); live.baseURL != DefaultBaseURL {
		t.Fatalf("an empty base URL is the live API: %s", live.baseURL)
	}
	_, err = New(packed("rt-bad"), fake.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_grant") {
		t.Fatalf("a refused refresh token must read as not_connected, got %v", err)
	}
	_, err = New("bare", fake.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "client id, client secret and refresh token") {
		t.Fatalf("one bare value names the credentials, got %v", err)
	}
	_, err = source.Describe("calls")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
}

func TestRefusalsReadTheStatus(t *testing.T) {
	fake, _ := fakeOutreach(t)
	source := New(packed("rt-good"), fake.URL)
	source.token = "at-stale"
	_, err := source.Page("prospects", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the access token") {
		t.Fatalf("a 401 must read as not_connected, got %v", err)
	}
	source.token = "at-slow"
	_, err = source.Page("prospects", nil, 0)
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"errors":[{"id":"forbidden","title":"Forbidden","detail":"missing scope prospects.read"}]}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "prospects.read") {
		t.Fatalf("a 403 names the detail, got %v", err)
	}
}

func TestDescribeFlattensProspectsWithCustomAttributes(t *testing.T) {
	fake, _ := fakeOutreach(t)
	description, err := New(packed("rt-good"), fake.URL).Describe("prospects")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || description.Counted || description.Label != "Prospects" || description.Hash != "2023-11-15T10:00:00.250Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["account_id"].Guess != "id" || byName["owner_id"].Guess != "id" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["email"].Samples; len(got) != 2 || got[0] != "Ann@Northwind.example" {
		t.Fatalf("the first email leads: %v", got)
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["phone"].Samples[0] != "+1 555 010 0000" || byName["tags"].Samples[0] != "finance,vip" {
		t.Fatalf("domains, phones and tags: %+v %+v %+v", byName["email_domain"], byName["phone"], byName["tags"])
	}
	if byName["account_id"].Samples[0] != "10" || byName["owner_id"].Samples[0] != "7" || byName["stage_id"].Samples[0] != "2" {
		t.Fatalf("relationships: %+v %+v %+v", byName["account_id"], byName["owner_id"], byName["stage_id"])
	}
	if byName["opted_out"].Samples[0] != "no" || byName["opted_out"].Samples[1] != "yes" || byName["created"].Samples[0] != "2023-11-14T22:13:20Z" {
		t.Fatalf("flags and times: %v %v", byName["opted_out"].Samples, byName["created"].Samples)
	}
	if byName["custom3"].Samples[0] != "gold" || byName["custom12"].Samples[0] != "west" || byName["custom3"].Filled != 1 {
		t.Fatalf("filled custom attributes are columns: %+v %+v", byName["custom3"], byName["custom12"])
	}
	if _, empty := byName["custom1"]; empty {
		t.Fatal("an empty custom attribute is no column")
	}
	names := []string{}
	for _, field := range description.Fields {
		if strings.HasPrefix(field.Name, "custom") {
			names = append(names, field.Name)
		}
	}
	if strings.Join(names, ",") != "custom3,custom12" {
		t.Fatalf("custom columns sort by number: %v", names)
	}
}

func TestPageWalksTheCursor(t *testing.T) {
	fake, calls := fakeOutreach(t)
	source := New(packed("rt-good"), fake.URL)
	first, err := source.Page("prospects", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T10:00:00.250Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	second, err := source.Page("prospects", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00.250Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.Contains(call, "/api/v2/prospects?") && strings.Contains(call, "page%5Bafter%5D=2") && strings.Contains(call, "page%5Bsize%5D=2") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not start after 2: %v", *calls)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "POST /oauth/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the source signs in once per process: %d", signIns)
	}

	accounts, err := source.Page("accounts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	row := accounts.Rows[0]
	if row["name"] != "Northwind Traders" || row["domain"] != "northwind.example" || row["employees"] != "120" || row["owner_id"] != "7" || row["custom2"] != "tier-1" {
		t.Fatalf("account row: %v", row)
	}
	sequences, err := source.Page("sequences", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if sequences.Rows[0]["enabled"] != "yes" || sequences.Rows[0]["steps"] != "5" || sequences.Rows[0]["reply_count"] != "3" {
		t.Fatalf("sequence row: %v", sequences.Rows[0])
	}
	if _, custom := sequences.Rows[0]["custom1"]; custom {
		t.Fatal("a sequence carries no custom attributes")
	}
	mailboxes, err := source.Page("mailboxes", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	if mailboxes.Rows[0]["email_domain"] != "muniment.example" || mailboxes.Rows[0]["user_id"] != "7" || mailboxes.Rows[0]["send_disabled"] != "no" {
		t.Fatalf("mailbox row: %v", mailboxes.Rows[0])
	}
}

func TestDeltaFiltersAfterTheMark(t *testing.T) {
	fake, calls := fakeOutreach(t)
	source := New(packed("rt-good"), fake.URL)
	delta, err := source.Delta("prospects", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00.250Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("prospects", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00.000Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00.250Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("accounts", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00.000Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.Contains(call, "/api/v2/prospects?") && strings.Contains(call, "filter%5BupdatedAt%5D=") && strings.Contains(call, "..inf") && strings.Contains(call, "sort=-updatedAt") {
			filtered++
		}
	}
	if filtered != 2 {
		t.Fatalf("each marked delta is one filtered list: %d in %v", filtered, *calls)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20.000Z") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339")
	}
	if nextCursor("https://api.outreach.io/api/v2/prospects?page%5Bafter%5D=abc&page%5Bsize%5D=2") != "abc" || nextCursor("") != "" {
		t.Fatal("the next cursor did not read")
	}
	if newestUpdated(nil, "2023-01-01T00:00:00.000Z") != "2023-01-01T00:00:00.000Z" {
		t.Fatal("the mark never moves backwards")
	}
	older := []record{{Attributes: map[string]any{"updatedAt": "2022-01-01T00:00:00.000Z"}}}
	if newestUpdated(older, "2023-01-01T00:00:00.000Z") != "2023-01-01T00:00:00.000Z" {
		t.Fatal("the mark never moves backwards")
	}
	columns := customColumns([]Row{{"custom12": "a", "id": "1"}, {"custom3": "b"}})
	if len(columns) != 2 || columns[0].Name != "custom3" || columns[1].Name != "custom12" {
		t.Fatalf("custom columns: %+v", columns)
	}
}
