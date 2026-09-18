package dynamics

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

var (
	filterClause = regexp.MustCompile(`^modifiedon gt (\S+)$`)
	pageSize     = regexp.MustCompile(`odata\.maxpagesize=(\d+)`)
)

// fakeDynamics grants a client credentials token for one tenant, answers
// the entity sets from fixtures through skip tokens in the next link,
// filters by change time, counts, and refuses a wrong token or a table the
// application user cannot read.
func fakeDynamics(t *testing.T) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	accounts := []map[string]any{
		{"accountid": "a1", "name": "Northwind Traders", "websiteurl": "https://www.northwind.example/about", "emailaddress1": "Ann@Northwind.example", "telephone1": "+1 555 010 0000", "industrycode": 6, "industrycode" + formatted: "Retail",
			"customertypecode": 3, "customertypecode" + formatted: "Customer", "statecode": 0, "statecode" + formatted: "Active", "numberofemployees": 250, "revenue": 1250000.5,
			"_transactioncurrencyid_value": "cur1", "_transactioncurrencyid_value" + formatted: "US Dollar", "address1_city": "Seattle", "address1_stateorprovince": "WA", "address1_postalcode": "98101", "address1_country": "US",
			"_primarycontactid_value": "c1", "_ownerid_value": "u1", "createdon": "2023-01-01T08:00:00Z", "modifiedon": "2023-11-15T11:00:00Z"},
		{"accountid": "a2", "name": "Contoso", "statecode": 0, "statecode" + formatted: "Active", "_parentaccountid_value": "a1", "_parentaccountid_value" + formatted: "Northwind Traders", "createdon": "2023-02-01T08:00:00Z", "modifiedon": "2023-11-14T11:00:00Z"},
		{"accountid": "a3", "name": "Fabrikam", "statecode": 1, "statecode" + formatted: "Inactive", "createdon": "2023-03-01T08:00:00Z", "modifiedon": "2023-11-13T11:00:00Z"},
	}
	contacts := []map[string]any{
		{"contactid": "c1", "firstname": "Ann", "lastname": "Lee", "fullname": "Ann Lee", "emailaddress1": "ann@northwind.example", "telephone1": "+1 555 010 0001", "jobtitle": "CFO", "_parentcustomerid_value": "a1", "_parentcustomerid_value" + formatted: "Northwind Traders",
			"statecode": 0, "statecode" + formatted: "Active", "_ownerid_value": "u1", "createdon": "2023-01-02T08:00:00Z", "modifiedon": "2023-11-12T11:00:00Z"},
	}
	opportunities := []map[string]any{
		{"opportunityid": "o1", "name": "Northwind renewal", "estimatedvalue": 12000.0, "_transactioncurrencyid_value": "cur1", "_transactioncurrencyid_value" + formatted: "US Dollar", "salesstage": 2, "salesstage" + formatted: "Propose", "statecode": 0, "statecode" + formatted: "Open",
			"statuscode": 1, "statuscode" + formatted: "In Progress", "closeprobability": 60, "estimatedclosedate": "2023-12-31", "_parentaccountid_value": "a1", "_customerid_value": "a1", "_ownerid_value": "u1", "createdon": "2023-10-01T08:00:00Z", "modifiedon": "2023-11-16T08:00:00Z"},
		{"opportunityid": "o2", "name": "Contoso pilot", "estimatedvalue": 500.0, "actualvalue": 450.0, "salesstage": 3, "salesstage" + formatted: "Close", "statecode": 1, "statecode" + formatted: "Won", "actualclosedate": "2023-11-10", "_parentaccountid_value": "a2", "createdon": "2023-10-02T08:00:00Z", "modifiedon": "2023-11-10T08:00:00Z"},
	}
	data := map[string][]map[string]any{"accounts": accounts, "contacts": contacts, "opportunities": opportunities}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery+" prefer="+r.Header.Get("Prefer"))
		if strings.HasSuffix(r.URL.Path, "/oauth2/v2.0/token") {
			_ = r.ParseForm()
			if r.URL.Path != "/tenant1/oauth2/v2.0/token" {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_request","error_description":"AADSTS90002: Tenant not found."}`))
				return
			}
			if r.Form.Get("grant_type") != "client_credentials" || r.Form.Get("client_id") != "app" || r.Form.Get("client_secret") != "hush" || r.Form.Get("scope") != "https://acme.crm.dynamics.com/.default" {
				w.WriteHeader(http.StatusUnauthorized)
				_, _ = w.Write([]byte(`{"error":"invalid_client","error_description":"AADSTS7000215: Invalid client secret provided."}`))
				return
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"token_type": "Bearer", "expires_in": 3599, "access_token": "session-1"})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":{"code":"0x80040217","message":"The user is not a member of the organization."}}`))
			return
		}
		if r.Header.Get("OData-Version") != "4.0" {
			t.Errorf("every read names the OData version")
		}
		set := strings.TrimPrefix(r.URL.Path, apiPath+"/")
		if set == "leads" {
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"error":{"code":"0x80040220","message":"Principal user is missing prvReadLead privilege."}}`))
			return
		}
		rows, ok := data[set]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":{"code":"0x80060888","message":"Resource not found for the segment."}}`))
			return
		}
		query := r.URL.Query()
		if query.Get("$select") == "" {
			t.Errorf("every read selects its attributes: %s", r.URL.RawQuery)
		}
		if match := filterClause.FindStringSubmatch(query.Get("$filter")); match != nil {
			kept := []map[string]any{}
			for _, row := range rows {
				if row["modifiedon"].(string) > match[1] {
					kept = append(kept, row)
				}
			}
			rows = kept
		}
		if query.Get("$orderby") == "modifiedon desc" {
			sorted := append([]map[string]any(nil), rows...)
			for i := range sorted {
				for j := i + 1; j < len(sorted); j++ {
					if sorted[j]["modifiedon"].(string) > sorted[i]["modifiedon"].(string) {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
			}
			rows = sorted
		}
		size := len(rows)
		if match := pageSize.FindStringSubmatch(r.Header.Get("Prefer")); match != nil {
			size, _ = strconv.Atoi(match[1])
		}
		if top, err := strconv.Atoi(query.Get("$top")); err == nil && top < size {
			size = top
		}
		from, _ := strconv.Atoi(query.Get("$skiptoken"))
		if from > len(rows) {
			from = len(rows)
		}
		end := from + size
		if end > len(rows) {
			end = len(rows)
		}
		answer := map[string]any{"@odata.context": "https://acme.crm.dynamics.com/api/data/v9.2/$metadata#" + set, "value": rows[from:end]}
		if query.Get("$count") == "true" {
			answer["@odata.count"] = len(rows)
		}
		if end < len(rows) && query.Get("$top") == "" {
			next := *r.URL
			next.Scheme, next.Host = "http", r.Host
			values := next.Query()
			values.Set("$skiptoken", strconv.Itoa(end))
			next.RawQuery = values.Encode()
			answer["@odata.nextLink"] = next.String()
		}
		_ = json.NewEncoder(w).Encode(answer)
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(tenant, secret string) string {
	body, _ := json.Marshal(map[string]string{"tenant_id": tenant, "client_id": "app", "client_secret": secret, "org_url": "https://acme.crm.dynamics.com/"})
	return string(body)
}

func TestObjectsSignInOnceAndProveTheEnvironment(t *testing.T) {
	server, calls := fakeDynamics(t)
	source := New(packed("tenant1", "hush"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 4 || objects[0].Name != "accounts" || objects[3].Label != "Opportunities" {
		t.Fatalf("objects: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns := 0
	for _, call := range *calls {
		if strings.Contains(call, "/oauth2/v2.0/token") {
			signIns++
		}
	}
	if signIns != 1 {
		t.Fatalf("the token is exchanged once per process: %d in %v", signIns, *calls)
	}
	if live := New(packed("tenant1", "hush"), ""); live.loginURL != DefaultLoginURL || live.orgURL != "https://acme.crm.dynamics.com" || live.scope != "https://acme.crm.dynamics.com/.default" {
		t.Fatalf("an empty base URL is the live login host and the environment: %s %s %s", live.loginURL, live.orgURL, live.scope)
	}
	_, err = New(packed("tenant1", "wrong"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_client") {
		t.Fatalf("a refused client secret must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New(packed("tenant2", "hush"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Tenant not found") {
		t.Fatalf("an unknown tenant reads as not_connected with the OAuth error, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "environment URL") {
		t.Fatalf("one bare value names the four credentials, got %v", err)
	}
	_, err = source.Describe("leads")
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "prvReadLead") {
		t.Fatalf("a table the application user cannot read must read as not_connected with the message, got %v", err)
	}
	_, err = source.Describe("cases")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" {
		t.Fatalf("an unknown object must read as unknown_object, got %v", err)
	}
	if err := refusal(http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a read must read as not_connected, got %v", err)
	}
	if err := refusal(http.StatusTooManyRequests, nil); err.(*Failure).Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
}

func TestDescribeCountsAndFlattensOpportunitiesWithMoneyAndStages(t *testing.T) {
	server, _ := fakeDynamics(t)
	description, err := New(packed("tenant1", "hush"), server.URL).Describe("opportunities")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 2 || !description.Counted || description.Label != "Opportunities" || description.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("description: %+v", description)
	}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		byName[field.Name] = field
	}
	if byName["account_id"].Guess != "id" || byName["amount"].Guess != "number" || byName["close_date"].Guess != "date" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["amount"].Samples; len(got) != 2 || got[0] != "12000.00" || got[1] != "500.00" {
		t.Fatalf("amounts read as decimals in the major unit: %v", got)
	}
	if got := byName["stage"].Samples; len(got) != 2 || got[0] != "proposal" || got[1] != "won" {
		t.Fatalf("stages fold from the state and the sales stage: %v", got)
	}
	if got := byName["stage_name"].Samples; len(got) != 2 || got[0] != "Propose" || got[1] != "Close" {
		t.Fatalf("the stage name reads from the formatted value: %v", got)
	}
	if byName["currency"].Samples[0] != "US Dollar" || byName["currency_id"].Samples[0] != "cur1" || byName["currency"].Filled != 1 {
		t.Fatalf("the currency lookup reads as its id and its name: %v %v", byName["currency"].Samples, byName["currency_id"].Samples)
	}
	if byName["actual_amount"].Samples[0] != "450.00" || byName["actual_amount"].Filled != 1 || byName["account_id"].Samples[1] != "a2" || byName["created"].Samples[0] != "2023-10-01T08:00:00Z" {
		t.Fatalf("actuals, lookups and times: %v %v %v", byName["actual_amount"].Samples, byName["account_id"].Samples, byName["created"].Samples)
	}
}

func TestPageWalksTheNextLinkAndFlattensAccountsAndContacts(t *testing.T) {
	server, calls := fakeDynamics(t)
	source := New(packed("tenant1", "hush"), server.URL)
	first, err := source.Page("accounts", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 3 || !first.Counted || first.Next == nil {
		t.Fatalf("first page: rows %d offset %d total %d next %v", len(first.Rows), first.Offset, first.Total, first.Next)
	}
	if !strings.Contains(first.Next.Token, "%24skiptoken=2") || first.Next.Offset != 2 || first.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["name"] != "Northwind Traders" || row["domain"] != "northwind.example" || row["email_domain"] != "northwind.example" || row["industry"] != "Retail" || row["type"] != "Customer" || row["status"] != "active" {
		t.Fatalf("account row: %v", row)
	}
	if row["annual_revenue"] != "1250000.50" || row["currency"] != "US Dollar" || row["currency_id"] != "cur1" || row["employees"] != "250" || row["state"] != "WA" || row["primary_contact_id"] != "c1" || row["owner_id"] != "u1" {
		t.Fatalf("account money, lookups and address: %v", row)
	}
	if first.Rows[1]["parent_id"] != "a1" || first.Rows[1]["domain"] != "" || first.Rows[1]["annual_revenue"] != "" {
		t.Fatalf("the parent lookup reads as its own column: %v", first.Rows[1])
	}
	second, err := source.Page("accounts", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Next != nil || second.Rows[0]["id"] != "a3" || second.Rows[0]["status"] != "inactive" {
		t.Fatalf("second page: rows %d offset %d total %d next %v", len(second.Rows), second.Offset, second.Total, second.Next)
	}
	if second.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("the mark keeps the newest change seen: %s", second.Hash)
	}
	walked := false
	for _, call := range *calls {
		if strings.Contains(call, "skiptoken=2") && strings.Contains(call, "odata.maxpagesize=2") {
			walked = true
		}
	}
	if !walked {
		t.Fatalf("the second page did not follow the next link with the page size: %v", *calls)
	}
	contacts, err := source.Page("contacts", nil, 0)
	if err != nil {
		t.Fatal(err)
	}
	item := contacts.Rows[0]
	if item["name"] != "Ann Lee" || item["account_id"] != "a1" || item["account_name"] != "Northwind Traders" || item["job_title"] != "CFO" || item["email_domain"] != "northwind.example" || item["status"] != "active" || item["modified"] != "2023-11-12T11:00:00Z" {
		t.Fatalf("contact row: %v", item)
	}
	if contacts.Next != nil || contacts.Total != 1 {
		t.Fatalf("one page of contacts ends the walk: %+v", contacts)
	}
}

func TestDeltaQueriesOnceAfterTheMark(t *testing.T) {
	server, calls := fakeDynamics(t)
	source := New(packed("tenant1", "hush"), server.URL)
	delta, err := source.Delta("accounts", &Cursor{Offset: 3, Hash: "2023-11-15T11:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "unchanged" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("accounts", &Cursor{Offset: 3, Hash: "2023-11-14T00:00:00Z"})
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-15T11:00:00Z" {
		t.Fatalf("delta: %+v", delta)
	}
	delta, err = source.Delta("opportunities", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || delta.Hash != "2023-11-16T08:00:00Z" {
		t.Fatalf("a first delta reads the newest change: %+v", delta)
	}
	queries, filtered := 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "%24orderby=modifiedon+desc") && strings.Contains(call, "%24top=1") {
			queries++
		}
		if strings.Contains(call, "%24filter=modifiedon+gt+2023-11-14T00%3A00%3A00Z") {
			filtered++
		}
	}
	if queries != 3 || filtered != 1 {
		t.Fatalf("each delta is one query and the mark travels in the filter: %d %d in %v", queries, filtered, *calls)
	}
}
