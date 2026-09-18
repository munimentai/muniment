package mailchimp

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"testing"
)

// fixtures is the fake account: the lists a test may change between calls.
type fixtures struct {
	members   map[string][]map[string]any
	campaigns []map[string]any
}

// fakeMailchimp answers the audiences, their merge fields, their members
// and the campaigns from fixtures, pages by offset with counts, filters
// members by last_changed, and refuses a wrong key.
func fakeMailchimp(t *testing.T) (*httptest.Server, *[]string, *fixtures) {
	t.Helper()
	var calls []string
	lists := []map[string]any{
		{"id": "list_b", "name": "Newsletter"},
		{"id": "list_a", "name": "Customers"},
	}
	merges := map[string][]map[string]any{
		"list_a": {
			{"merge_id": 1.0, "tag": "FNAME", "name": "First Name", "type": "text"},
			{"merge_id": 2.0, "tag": "LNAME", "name": "Last Name", "type": "text"},
			{"merge_id": 3.0, "tag": "PHONE", "name": "Phone", "type": "phone"},
			{"merge_id": 4.0, "tag": "COMPANY", "name": "Company", "type": "text"},
			{"merge_id": 5.0, "tag": "MMERGE5", "name": "Setup fee (USD)", "type": "number"},
		},
		"list_b": {
			{"merge_id": 1.0, "tag": "FNAME", "name": "First Name", "type": "text"},
			{"merge_id": 2.0, "tag": "LNAME", "name": "Last Name", "type": "text"},
		},
	}
	data := &fixtures{
		members: map[string][]map[string]any{
			"list_a": {
				{"id": "m1", "email_address": "Ann@Northwind.example", "full_name": "Ann Lee", "status": "subscribed", "email_type": "html", "member_rating": 4.0, "language": "en", "vip": true, "source": "Import",
					"merge_fields": map[string]any{"FNAME": "Ann", "LNAME": "Lee", "PHONE": "+1 555 010 0000", "COMPANY": "Northwind Traders", "MMERGE5": 99.5, "ADDRESS": map[string]any{"city": "Seattle"}},
					"tags":         []any{map[string]any{"id": 2.0, "name": "vip"}, map[string]any{"id": 1.0, "name": "finance"}}, "interests": map[string]any{"i1": true, "i2": false},
					"stats": map[string]any{"avg_open_rate": 0.5, "avg_click_rate": 0.1}, "location": map[string]any{"country_code": "US", "region": "WA", "timezone": "America/Los_Angeles"},
					"list_id": "list_a", "web_id": 100.0, "contact_id": "c100", "timestamp_signup": "2023-11-14T22:13:20+00:00", "timestamp_opt": "2023-11-14T22:14:20+00:00", "last_changed": "2023-11-15T10:00:00+00:00"},
				{"id": "m2", "email_address": "bo@fabrikam.example", "full_name": "", "status": "unsubscribed", "unsubscribe_reason": "N/A (Unsubscribed by an admin)", "vip": false, "merge_fields": map[string]any{"FNAME": "Bo", "LNAME": "Fabrik"}, "tags": []any{}, "list_id": "list_a", "last_changed": "2023-11-13T09:00:00+00:00"},
				{"id": "m3", "email_address": "cy@contoso.example", "status": "pending", "merge_fields": map[string]any{}, "list_id": "list_a", "last_changed": "2023-11-12T09:00:00+00:00"},
			},
			"list_b": {
				{"id": "m4", "email_address": "di@northwind.example", "full_name": "Di North", "status": "subscribed", "merge_fields": map[string]any{"FNAME": "Di", "LNAME": "North"}, "list_id": "list_b", "last_changed": "2023-11-15T11:00:00+00:00"},
			},
		},
		campaigns: []map[string]any{
			{"id": "c2", "web_id": 2.0, "type": "plaintext", "status": "save", "settings": map[string]any{"title": "Draft"}, "recipients": map[string]any{}, "emails_sent": 0.0, "create_time": "2023-11-10T12:00:00+00:00", "send_time": ""},
			{"id": "c1", "web_id": 1.0, "type": "regular", "status": "sent", "content_type": "template", "archive_url": "https://eepurl.example/c1",
				"settings":   map[string]any{"title": "November news", "subject_line": "News", "preview_text": "What is new", "from_name": "Ann", "reply_to": "ann@northwind.example"},
				"recipients": map[string]any{"list_id": "list_a", "list_name": "Customers", "recipient_count": 3.0, "segment_text": ""}, "emails_sent": 3.0,
				"report_summary": map[string]any{"opens": 5.0, "unique_opens": 2.0, "open_rate": 0.66, "clicks": 1.0, "click_rate": 0.33}, "create_time": "2023-11-15T12:00:00+00:00", "send_time": "2023-11-15T13:00:00+00:00"},
		},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.URL.Path+"?"+r.URL.RawQuery)
		_, key, _ := r.BasicAuth()
		switch key {
		case "key-good-us21":
		case "key-slow-us21":
			w.WriteHeader(http.StatusTooManyRequests)
			_, _ = w.Write([]byte(`{"type":"https://mailchimp.com/developer/marketing/docs/errors/","title":"Too Many Requests","status":429,"detail":"You have exceeded the limit of 10 simultaneous connections."}`))
			return
		default:
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"type":"https://mailchimp.com/developer/marketing/docs/errors/","title":"API Key Invalid","status":401,"detail":"Your API key may be invalid, or you've attempted to access the wrong datacenter."}`))
			return
		}
		query := r.URL.Query()
		notFound := func() {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"type":"https://mailchimp.com/developer/marketing/docs/errors/","title":"Resource Not Found","status":404,"detail":"The requested resource could not be found."}`))
		}
		parts := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
		switch {
		case r.URL.Path == "/3.0/lists":
			page, total := paginate(lists, query)
			_ = json.NewEncoder(w).Encode(map[string]any{"lists": page, "total_items": total})
		case len(parts) == 3 && parts[1] == "lists":
			for _, l := range lists {
				if l["id"] == parts[2] {
					_ = json.NewEncoder(w).Encode(l)
					return
				}
			}
			notFound()
		case len(parts) == 4 && parts[1] == "lists" && parts[3] == "merge-fields":
			definitions, ok := merges[parts[2]]
			if !ok {
				notFound()
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"merge_fields": definitions, "list_id": parts[2], "total_items": len(definitions)})
		case len(parts) == 4 && parts[1] == "lists" && parts[3] == "members":
			rows, ok := data.members[parts[2]]
			if !ok {
				notFound()
				return
			}
			if since := query.Get("since_last_changed"); since != "" {
				kept := []map[string]any{}
				for _, row := range rows {
					if changed(row["last_changed"].(string)) > changed(since) {
						kept = append(kept, row)
					}
				}
				rows = kept
			}
			if query.Get("sort_field") == "last_changed" {
				rows = sorted(rows, "last_changed", query.Get("sort_dir") == "DESC")
			}
			page, total := paginate(rows, query)
			_ = json.NewEncoder(w).Encode(map[string]any{"members": page, "list_id": parts[2], "total_items": total})
		case r.URL.Path == "/3.0/campaigns":
			rows := data.campaigns
			if query.Get("sort_field") == "create_time" {
				rows = sorted(rows, "create_time", query.Get("sort_dir") == "DESC")
			}
			page, total := paginate(rows, query)
			_ = json.NewEncoder(w).Encode(map[string]any{"campaigns": page, "total_items": total})
		default:
			notFound()
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls, data
}

// changed folds an ISO timestamp with any offset into a form that sorts.
func changed(value string) string {
	return formatTime(value)
}

func sorted(rows []map[string]any, key string, descending bool) []map[string]any {
	out := append([]map[string]any(nil), rows...)
	for i := 0; i < len(out); i++ {
		for j := i + 1; j < len(out); j++ {
			a, b := changed(out[i][key].(string)), changed(out[j][key].(string))
			if (descending && b > a) || (!descending && b < a) {
				out[i], out[j] = out[j], out[i]
			}
		}
	}
	return out
}

// paginate cuts one page by count from the offset and answers the total.
func paginate(rows []map[string]any, query url.Values) ([]map[string]any, int) {
	count, _ := strconv.Atoi(query.Get("count"))
	if count <= 0 {
		count = 10
	}
	start, _ := strconv.Atoi(query.Get("offset"))
	if start > len(rows) {
		start = len(rows)
	}
	end := start + count
	if end > len(rows) {
		end = len(rows)
	}
	return rows[start:end], len(rows)
}

func TestObjectsProveTheKeyAndListAudiences(t *testing.T) {
	server, calls, _ := fakeMailchimp(t)
	source := New("key-good-us21", server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 3 || objects[0].Name != "list_a" || objects[0].Label != "Customers" || objects[1].Name != "list_b" || objects[2].Name != "campaigns" || objects[2].Label != "Campaigns" {
		t.Fatalf("objects: %+v", objects)
	}
	if len(*calls) != 1 || !strings.HasPrefix((*calls)[0], "/3.0/lists?") {
		t.Fatalf("the key was proved with one walk of the audiences: %v", *calls)
	}
	if _, err := source.Objects(); err != nil || len(*calls) != 1 {
		t.Fatalf("the audiences read once per source: %v", *calls)
	}
	if live := New("abc-us21", ""); live.base != "https://us21.api.mailchimp.com" {
		t.Fatalf("the key's suffix names the data center: %s", live.base)
	}
	if packed := New(`{"token":" abc-us6 "}`, ""); packed.base != "https://us6.api.mailchimp.com" || packed.key != "abc-us6" {
		t.Fatalf("a packed secret reads the same: %s %s", packed.base, packed.key)
	}
	_, err = New("nodash", "").Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "names no data center") {
		t.Fatalf("a key without a data center must read as not_connected, got %v", err)
	}
	_, err = New("key-bad-us21", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "refused the API key") {
		t.Fatalf("a refused key must read as not_connected, got %v", err)
	}
	_, err = New("key-slow-us21", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "rate_limited" {
		t.Fatalf("a rate limit must read as rate_limited, got %v", err)
	}
	_, err = source.Describe("list_zz")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown audience must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusForbidden, []byte(`{"title":"Forbidden","status":403,"detail":"This account has been deactivated."}`)); err == nil || err.(*Failure).Code != "not_connected" || !strings.Contains(err.Error(), "deactivated") {
		t.Fatalf("a 403 names the detail, got %v", err)
	}
}

func TestDescribeCountsAndFlattensMembersWithMergeFields(t *testing.T) {
	server, calls, _ := fakeMailchimp(t)
	source := New("key-good-us21", server.URL)
	description, err := source.Describe("list_a")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Customers" || description.Source != "mailchimp" || description.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	if len(description.Fields) != len(memberFields)+2 {
		t.Fatalf("the core columns and the two extra merge fields: %d", len(description.Fields))
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["id"].Guess != "id" || byName["email"].Guess != "email" || byName["email_domain"].Guess != "domain" || byName["setup_fee_usd"].Guess != "number" || byName["company"].Guess != "string" {
		t.Fatalf("guesses: %+v", byName)
	}
	if byName["email_domain"].Samples[0] != "northwind.example" || byName["first_name"].Samples[0] != "Ann" || byName["last_name"].Samples[1] != "Fabrik" || byName["phone"].Samples[0] != "+1 555 010 0000" {
		t.Fatalf("emails and merge fields: %+v %+v", byName["email_domain"], byName["first_name"])
	}
	if byName["name"].Samples[0] != "Ann Lee" || byName["name"].Samples[1] != "Bo Fabrik" || byName["name"].Samples[2] != "cy@contoso.example" {
		t.Fatalf("names fall back to the merge fields, then the email: %v", byName["name"].Samples)
	}
	if byName["tags"].Samples[0] != "finance,vip" || byName["tag_ids"].Samples[0] != "1,2" || byName["interests"].Samples[0] != "i1" || byName["vip"].Samples[0] != "yes" || byName["vip"].Samples[1] != "no" {
		t.Fatalf("tags, interests and flags: %v %v %v", byName["tags"].Samples, byName["interests"].Samples, byName["vip"].Samples)
	}
	if byName["country"].Samples[0] != "US" || byName["rating"].Samples[0] != "4" || byName["open_rate"].Samples[0] != "0.5" || byName["signed_up"].Samples[0] != "2023-11-14T22:13:20Z" || byName["modified"].Samples[0] != "2023-11-15T10:00:00Z" {
		t.Fatalf("location, stats and times: %v %v %v", byName["country"].Samples, byName["rating"].Samples, byName["signed_up"].Samples)
	}
	if byName["company"].Samples[0] != "Northwind Traders" || byName["company"].Filled != 1 || byName["setup_fee_usd"].Samples[0] != "99.5" {
		t.Fatalf("the merge field is a column named as the account named it: %+v %+v", byName["company"], byName["setup_fee_usd"])
	}
	if _, address := byName["address"]; address {
		t.Fatal("a merge field with no definition is no column")
	}
	reads, definitions := 0, 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/3.0/lists/list_a?") {
			reads++
		}
		if strings.HasPrefix(call, "/3.0/lists/list_a/merge-fields") {
			definitions++
		}
	}
	if reads != 1 || definitions != 1 {
		t.Fatalf("an audience Objects never listed reads once, and its merge fields read once: %v", *calls)
	}

	campaigns, err := source.Describe("campaigns")
	if err != nil {
		t.Fatal(err)
	}
	if campaigns.Rows != 2 || !campaigns.Counted || campaigns.Label != "Campaigns" || len(campaigns.Hash) != 64 {
		t.Fatalf("campaigns: %+v", campaigns)
	}
	byName = map[string]FieldDescription{}
	for _, field := range campaigns.Fields {
		byName[field.Name] = field
	}
	if byName["subject"].Samples[0] != "News" || byName["list_id"].Samples[0] != "list_a" || byName["opens"].Samples[0] != "5" || byName["sent"].Samples[0] != "2023-11-15T13:00:00Z" || byName["reply_to"].Guess != "email" {
		t.Fatalf("campaign fields: %v %v %v", byName["subject"].Samples, byName["list_id"].Samples, byName["sent"].Samples)
	}
	if byName["title"].Samples[1] != "Draft" || byName["sent"].Filled != 1 {
		t.Fatalf("the newest campaign comes first and a draft has no send time: %v %+v", byName["title"].Samples, byName["sent"])
	}
}

func TestPageWalksOffsets(t *testing.T) {
	server, calls, _ := fakeMailchimp(t)
	source := New("key-good-us21", server.URL)
	first, err := source.Page("list_a", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: %+v", first)
	}
	if first.Next.Token != "2" || first.Next.Offset != 2 || first.Hash != "2023-11-15T10:00:00Z" || first.Rows[0]["id"] != "m1" || first.Rows[1]["id"] != "m2" {
		t.Fatalf("next cursor: %+v %v", first.Next, first.Rows)
	}
	second, err := source.Page("list_a", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "m3" {
		t.Fatalf("second page: %+v", second)
	}
	if second.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	found := false
	for _, call := range *calls {
		if strings.HasPrefix(call, "/3.0/lists/list_a/members?") && strings.Contains(call, "offset=2") && strings.Contains(call, "count=2") && strings.Contains(call, "exclude_fields=members._links") {
			found = true
		}
	}
	if !found {
		t.Fatalf("the second page did not ask for offset 2: %v", *calls)
	}

	campaigns, err := source.Page("campaigns", nil, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(campaigns.Rows) != 1 || campaigns.Total != 2 || campaigns.Next == nil || campaigns.Next.Token != "1" || len(campaigns.Hash) != 64 || campaigns.Rows[0]["id"] != "c1" {
		t.Fatalf("campaigns first page: %+v", campaigns)
	}
	row := campaigns.Rows[0]
	if row["title"] != "November news" || row["from_name"] != "Ann" || row["recipients"] != "3" || row["click_rate"] != "0.33" || row["status"] != "sent" || row["created"] != "2023-11-15T12:00:00Z" {
		t.Fatalf("campaign row: %v", row)
	}
	rest, err := source.Page("campaigns", campaigns.Next, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(rest.Rows) != 1 || rest.Offset != 1 || rest.Next != nil || rest.Rows[0]["id"] != "c2" || rest.Hash != campaigns.Hash {
		t.Fatalf("campaigns second page carries the mark: %+v", rest)
	}
}

func TestDeltaFiltersAfterTheMark(t *testing.T) {
	server, calls, data := fakeMailchimp(t)
	source := New("key-good-us21", server.URL)
	delta, err := source.Delta("list_a", &Cursor{Offset: 3, Hash: "2023-11-15T10:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("list_a", &Cursor{Offset: 3, Hash: "2023-11-13T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T10:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("list_b", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	filtered := 0
	for _, call := range *calls {
		if strings.HasPrefix(call, "/3.0/lists/list_a/members?") && strings.Contains(call, "since_last_changed=") && strings.Contains(call, "count=1") && strings.Contains(call, "sort_dir=DESC") {
			filtered++
		}
	}
	if filtered != 2 {
		t.Fatalf("each marked delta is one filtered list: %d in %v", filtered, *calls)
	}

	first, err := source.Page("campaigns", nil, PageLimit)
	if err != nil {
		t.Fatal(err)
	}
	delta, err = source.Delta("campaigns", &Cursor{Offset: 2, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("an unchanged campaigns page keeps its hash: %+v", delta)
	}
	data.campaigns[1]["status"] = "paused"
	delta, err = source.Delta("campaigns", &Cursor{Offset: 2, Hash: first.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash == first.Hash || len(delta.Hash) != 64 {
		t.Fatalf("a changed campaign changes the hash: %+v", delta)
	}
}

func TestValuesRead(t *testing.T) {
	if formatTime("2023-11-14T22:13:20+00:00") != "2023-11-14T22:13:20Z" || formatTime("2023-11-14T17:13:20-05:00") != "2023-11-14T22:13:20Z" || formatTime("") != "" {
		t.Fatal("times did not read as RFC 3339 in UTC")
	}
	if columnName("Setup fee (USD)") != "setup_fee_usd" || columnName(" Company ") != "company" {
		t.Fatal("column names did not fold")
	}
	if newestChanged(nil, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" || newestChanged([]record{{"last_changed": "2022-01-01T00:00:00+00:00"}}, "2023-01-01T00:00:00Z") != "2023-01-01T00:00:00Z" {
		t.Fatal("the mark never moves backwards")
	}
	if mailchimpMessage([]byte(`{"title":"Invalid Resource","status":400}`)) != "Invalid Resource" || mailchimpMessage([]byte(`oops`)) != "oops" {
		t.Fatal("a message reads its detail, then its title, then the body")
	}
	if hashRows(nil) != hashRows([]Row{}) || hashRows([]Row{{"a": "1"}}) == hashRows([]Row{{"a": "2"}}) {
		t.Fatal("the hash did not follow the rows")
	}
}
