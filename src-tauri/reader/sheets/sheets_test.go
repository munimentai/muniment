package sheets

import (
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

// account is one throwaway service account: its RSA key and the JSON text
// Google would download for it.
type account struct {
	email string
	key   *rsa.PrivateKey
}

func newAccount(t *testing.T, email string) account {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	return account{email: email, key: key}
}

// json writes the key file Google downloads, with the token endpoint on
// the fake.
func (a account) json(tokenURL string) string {
	der, _ := x509.MarshalPKCS8PrivateKey(a.key)
	encoded := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: der})
	body, _ := json.Marshal(map[string]string{"type": "service_account", "client_email": a.email, "private_key": string(encoded), "token_uri": tokenURL})
	return string(body)
}

var rowsRange = regexp.MustCompile(`^(\d+):(\d+)$`)

// fakeSheets signs a service account in when its JWT verifies against the
// one key it knows, lists a spreadsheet's tabs, answers values by range,
// and refuses a wrong token, a spreadsheet the account cannot open, and
// one that does not exist.
func fakeSheets(t *testing.T, known account) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	leads := [][]any{
		{"Name", "Email", "Amount", "", "Name"},
		{"Northwind Traders", "ann@northwind.example", "1,200.50", "x", "dup"},
		{"Contoso", "bob@contoso.example", "50"},
		{"", "", ""},
		{"Fabrikam", "cy@fabrikam.example", "12"},
	}
	notes := [][]any{{"Note", "Done"}, {"Call Ann", "TRUE"}, {"Send invoice", "FALSE"}}
	tabs := map[string][][]any{"Leads": leads, "Notes": notes}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls = append(calls, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
		if r.URL.Path == "/token" {
			_ = r.ParseForm()
			if r.Form.Get("grant_type") != "urn:ietf:params:oauth:grant-type:jwt-bearer" {
				t.Errorf("the sign-in is a JWT bearer grant: %s", r.Form.Get("grant_type"))
			}
			parts := strings.Split(r.Form.Get("assertion"), ".")
			if len(parts) != 3 {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_grant","error_description":"Invalid JWT."}`))
				return
			}
			digest := sha256.Sum256([]byte(parts[0] + "." + parts[1]))
			signature, _ := base64.RawURLEncoding.DecodeString(parts[2])
			if rsa.VerifyPKCS1v15(&known.key.PublicKey, crypto.SHA256, digest[:], signature) != nil {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":"invalid_grant","error_description":"Invalid JWT Signature."}`))
				return
			}
			claimText, _ := base64.RawURLEncoding.DecodeString(parts[1])
			var claims map[string]any
			_ = json.Unmarshal(claimText, &claims)
			if claims["iss"] != known.email || claims["scope"] != Scope || claims["aud"] != "http://"+r.Host+"/token" {
				t.Errorf("the claims name the account, the read-only scope and the token endpoint: %v", claims)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "session-1", "token_type": "Bearer", "expires_in": 3599})
			return
		}
		if r.Header.Get("Authorization") != "Bearer session-1" {
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write([]byte(`{"error":{"code":401,"message":"Request had invalid authentication credentials.","status":"UNAUTHENTICATED"}}`))
			return
		}
		switch {
		case r.URL.Path == "/v4/spreadsheets/private1":
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}`))
		case r.URL.Path == "/v4/spreadsheets/sheet1":
			_ = json.NewEncoder(w).Encode(map[string]any{"properties": map[string]any{"title": "Pipeline"}, "sheets": []any{
				map[string]any{"properties": map[string]any{"sheetId": 0, "title": "Leads", "sheetType": "GRID"}},
				map[string]any{"properties": map[string]any{"sheetId": 12, "title": "Notes", "sheetType": "GRID"}},
				map[string]any{"properties": map[string]any{"sheetId": 13, "title": "Chart", "sheetType": "OBJECT"}},
			}})
		case strings.HasPrefix(r.URL.Path, "/v4/spreadsheets/sheet1/values/"):
			target := strings.TrimPrefix(r.URL.Path, "/v4/spreadsheets/sheet1/values/")
			title, rows := target, ""
			if bang := strings.LastIndex(target, "!"); bang >= 0 {
				title, rows = target[:bang], target[bang+1:]
			}
			title = strings.ReplaceAll(strings.Trim(title, "'"), "''", "'")
			values, ok := tabs[title]
			if !ok {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(`{"error":{"code":400,"message":"Unable to parse range","status":"INVALID_ARGUMENT"}}`))
				return
			}
			if match := rowsRange.FindStringSubmatch(rows); match != nil {
				from, _ := strconv.Atoi(match[1])
				to, _ := strconv.Atoi(match[2])
				if from < 1 {
					from = 1
				}
				if from > len(values) {
					from = len(values) + 1
				}
				if to > len(values) {
					to = len(values)
				}
				if from > to {
					values = nil
				} else {
					values = values[from-1 : to]
				}
			} else if rows != "" {
				t.Errorf("a range names whole rows: %s", rows)
			}
			for len(values) > 0 && len(values[len(values)-1]) == 0 {
				values = values[:len(values)-1]
			}
			answer := map[string]any{"range": target, "majorDimension": "ROWS"}
			if len(values) > 0 {
				answer["values"] = values
			}
			_ = json.NewEncoder(w).Encode(answer)
		default:
			w.WriteHeader(http.StatusNotFound)
			_, _ = w.Write([]byte(`{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}`))
		}
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func packed(keyText, spreadsheet string) string {
	body, _ := json.Marshal(map[string]string{"key": keyText, "spreadsheet_id": spreadsheet})
	return string(body)
}

func TestObjectsSignInOnceAndListTheGridTabs(t *testing.T) {
	reader := newAccount(t, "reader@acme.iam.gserviceaccount.com")
	server, calls := fakeSheets(t, reader)
	source := New(packed(reader.json(server.URL+"/token"), "https://docs.google.com/spreadsheets/d/sheet1/edit#gid=0"), server.URL)
	objects, err := source.Objects()
	if err != nil {
		t.Fatal(err)
	}
	if len(objects) != 2 || objects[0].Name != "Leads" || objects[1].Label != "Notes" {
		t.Fatalf("objects list the grid tabs and skip the chart: %+v", objects)
	}
	if _, err := source.Objects(); err != nil {
		t.Fatal(err)
	}
	signIns, listings := 0, 0
	for _, call := range *calls {
		if strings.Contains(call, "/token") {
			signIns++
		}
		if strings.Contains(call, "/v4/spreadsheets/sheet1?") {
			listings++
		}
	}
	if signIns != 1 || listings != 1 {
		t.Fatalf("the token is exchanged once and the tabs are listed once per process: %d %d in %v", signIns, listings, *calls)
	}
	if live := New(packed(reader.json(""), "sheet1"), ""); live.baseURL != DefaultBaseURL || live.tokenURL != DefaultTokenURL || live.spreadsheet != "sheet1" {
		t.Fatalf("an empty base URL is the live API and a key without a token URI signs in at the default: %s %s %s", live.baseURL, live.tokenURL, live.spreadsheet)
	}
	stranger := newAccount(t, "gone@acme.iam.gserviceaccount.com")
	_, err = New(packed(stranger.json(server.URL+"/token"), "sheet1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "invalid_grant") {
		t.Fatalf("a key Google does not know must read as not_connected with the OAuth error, got %v", err)
	}
	_, err = New(packed(reader.json(server.URL+"/token"), "private1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "Share the spreadsheet with reader@acme.iam.gserviceaccount.com") {
		t.Fatalf("a spreadsheet the account cannot open names the account to share with, got %v", err)
	}
	_, err = New(packed(reader.json(server.URL+"/token"), "missing1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "no spreadsheet missing1") {
		t.Fatalf("a spreadsheet that does not exist reads as not_connected, got %v", err)
	}
	_, err = New(packed(`{"client_email":"x@y","private_key":"not a key"}`, "sheet1"), server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "no PEM private key") {
		t.Fatalf("a key without PEM reads as not_connected, got %v", err)
	}
	_, err = New("just-a-token", server.URL).Objects()
	if failure, ok := err.(*Failure); !ok || failure.Code != "not_connected" || !strings.Contains(failure.Message, "spreadsheet id") {
		t.Fatalf("one bare value names the two credentials, got %v", err)
	}
	_, err = source.Describe("Deals")
	if failure, ok := err.(*Failure); !ok || failure.Code != "unknown_object" || !strings.Contains(failure.Message, "Leads, Notes") {
		t.Fatalf("an unknown tab must read as unknown_object naming the tabs, got %v", err)
	}
	if err := source.refusal(http.StatusUnauthorized, nil); err.(*Failure).Code != "not_connected" {
		t.Fatalf("a 401 on a read must read as not_connected, got %v", err)
	}
	if err := source.refusal(http.StatusTooManyRequests, nil); err.(*Failure).Code != "rate_limited" {
		t.Fatalf("a 429 must read as rate_limited, got %v", err)
	}
}

func TestDescribeReadsTheHeaderAndGuessesTypes(t *testing.T) {
	reader := newAccount(t, "reader@acme.iam.gserviceaccount.com")
	server, _ := fakeSheets(t, reader)
	source := New(packed(reader.json(server.URL+"/token"), "sheet1"), server.URL)
	description, err := source.Describe("Leads")
	if err != nil {
		t.Fatal(err)
	}
	if description.Rows != 3 || !description.Counted || description.Label != "Leads" || description.Hash == "" {
		t.Fatalf("description counts the filled rows: %+v", description)
	}
	names := []string{}
	byName := map[string]FieldDescription{}
	for _, field := range description.Fields {
		names = append(names, field.Name)
		byName[field.Name] = field
	}
	if strings.Join(names, ",") != "row,name,email,amount,column_d,name_2" {
		t.Fatalf("columns fold to snake_case, an empty header names its letter and a repeat is suffixed: %v", names)
	}
	if byName["row"].Guess != "id" || byName["name"].Guess != "string" || byName["email"].Guess != "email" || byName["amount"].Guess != "number" {
		t.Fatalf("guesses: %+v", byName)
	}
	if got := byName["row"].Samples; len(got) != 3 || got[0] != "2" || got[1] != "3" || got[2] != "5" {
		t.Fatalf("the row number is the id and an empty row is skipped: %v", got)
	}
	if got := byName["amount"].Samples; len(got) != 3 || got[0] != "1,200.50" || byName["column_d"].Filled != 1 {
		t.Fatalf("cells read as the sheet shows them: %v %d", got, byName["column_d"].Filled)
	}
	notes, err := source.Describe("12")
	if err != nil {
		t.Fatal(err)
	}
	if notes.Object != "12" || notes.Label != "Notes" || notes.Rows != 2 {
		t.Fatalf("a tab reads by its id too: %+v", notes)
	}
	for _, field := range notes.Fields {
		if field.Name == "done" && field.Guess != "boolean" {
			t.Fatalf("TRUE and FALSE cells guess as boolean: %+v", field)
		}
	}
}

func TestPageWalksTheRowsAndKeepsTheHash(t *testing.T) {
	reader := newAccount(t, "reader@acme.iam.gserviceaccount.com")
	server, calls := fakeSheets(t, reader)
	source := New(packed(reader.json(server.URL+"/token"), "sheet1"), server.URL)
	first, err := source.Page("Leads", nil, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(first.Rows) != 2 || first.Offset != 0 || first.Total != 2 || first.Counted || first.Next == nil || first.Hash == "" {
		t.Fatalf("first page: rows %d offset %d total %d next %v hash %q", len(first.Rows), first.Offset, first.Total, first.Next, first.Hash)
	}
	if first.Next.Token != "4" || first.Next.Offset != 2 || first.Next.Hash != first.Hash {
		t.Fatalf("next cursor: %+v", first.Next)
	}
	row := first.Rows[0]
	if row["row"] != "2" || row["name"] != "Northwind Traders" || row["email"] != "ann@northwind.example" || row["amount"] != "1,200.50" || row["column_d"] != "x" || row["name_2"] != "dup" {
		t.Fatalf("first row: %v", row)
	}
	if first.Rows[1]["row"] != "3" || first.Rows[1]["column_d"] != "" {
		t.Fatalf("a short row fills its missing cells with nothing: %v", first.Rows[1])
	}
	second, err := source.Page("Leads", first.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(second.Rows) != 1 || second.Offset != 2 || second.Total != 3 || second.Rows[0]["row"] != "5" || second.Rows[0]["name"] != "Fabrikam" || second.Hash != first.Hash {
		t.Fatalf("second page skips the empty row and keeps the hash: rows %d offset %d total %d hash %q", len(second.Rows), second.Offset, second.Total, second.Hash)
	}
	if second.Next == nil || second.Next.Token != "6" {
		t.Fatalf("a full range means another may follow: %+v", second.Next)
	}
	end, err := source.Page("Leads", second.Next, 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(end.Rows) != 0 || end.Next != nil || end.Offset != 3 {
		t.Fatalf("an empty range ends the walk: %+v", end)
	}
	whole, ranged := 0, false
	for _, call := range *calls {
		if strings.Contains(call, "/values/'Leads'?") {
			whole++
		}
		if strings.Contains(call, "/values/'Leads'!4:5?") {
			ranged = true
		}
	}
	if whole != 1 || !ranged {
		t.Fatalf("the first page reads the whole tab once and a later page reads its own range: %d %v in %v", whole, ranged, *calls)
	}
}

func TestDeltaComparesTheHashOfTheValues(t *testing.T) {
	reader := newAccount(t, "reader@acme.iam.gserviceaccount.com")
	server, _ := fakeSheets(t, reader)
	source := New(packed(reader.json(server.URL+"/token"), "sheet1"), server.URL)
	delta, err := source.Delta("Leads", nil)
	if err != nil {
		t.Fatal(err)
	}
	if delta.State != "changed" || len(delta.Hash) != 64 {
		t.Fatalf("a first delta is changed with the SHA-256 of the values: %+v", delta)
	}
	same, err := source.Delta("Leads", &Cursor{Offset: 3, Hash: delta.Hash})
	if err != nil {
		t.Fatal(err)
	}
	if same.State != "unchanged" {
		t.Fatalf("the same values are unchanged: %+v", same)
	}
	moved, err := source.Delta("Leads", &Cursor{Offset: 3, Hash: "stale"})
	if err != nil {
		t.Fatal(err)
	}
	if moved.State != "changed" || moved.Hash != delta.Hash {
		t.Fatalf("another mark is changed with the current hash: %+v", moved)
	}
	other, err := source.Delta("Notes", nil)
	if err != nil {
		t.Fatal(err)
	}
	if other.Hash == delta.Hash {
		t.Fatalf("each tab has its own hash")
	}
}
