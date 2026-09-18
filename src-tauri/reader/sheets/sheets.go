// Package sheets reads one Google spreadsheet behind the reader contract:
// each sheet tab is one object, its first row is the header, and every row
// below flattens to text by the header's column names. It uses the Sheets
// REST API over net/http alone, signs in once per process by signing a JWT
// with the service account's RSA key through the standard library, and it
// writes nothing back. Backfill walks the rows by range, and Delta is the
// hash of the tab's values, because Sheets cannot filter by change time.
package sheets

import (
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is the Sheets API host. A test points the reader elsewhere.
const DefaultBaseURL = "https://sheets.googleapis.com"

// DefaultTokenURL is where a service account key without a token URI signs
// in.
const DefaultTokenURL = "https://oauth2.googleapis.com/token"

// Scope is the one read-only scope the reader asks for.
const Scope = "https://www.googleapis.com/auth/spreadsheets.readonly"

// PageLimit is the most rows one Page call reads.
const PageLimit = 1000

// sampleRows is how many rows Describe reads for its samples.
const sampleRows = 100

// The wire shapes come from the contract package, so the runtime reads one
// set of fields whatever the source.
type (
	Failure          = contract.Failure
	Cursor           = contract.Cursor
	ObjectInfo       = contract.ObjectInfo
	FieldDescription = contract.FieldDescription
	Description      = contract.Description
	Row              = contract.Row
	Page             = contract.Page
	Delta            = contract.Delta
)

// Source is one spreadsheet reached through one service account.
type Source struct {
	spreadsheet string
	email       string
	privateKey  string
	tokenURL    string
	baseURL     string
	client      *http.Client
	token       string
	sheets      []sheet
	listed      bool
	now         func() time.Time
}

// sheet is one tab as an object: its title, which names the values range,
// and its numeric id.
type sheet struct {
	id    string
	title string
}

// New opens a source on the credentials the panel packs: the service
// account key as the JSON text Google downloads and the spreadsheet id.
// An empty base URL is the live API.
func New(secret, baseURL string) *Source {
	credentials := contract.Credentials(secret)
	keyText := credentials["key"]
	if keyText == "" {
		keyText = credentials["token"]
	}
	var key struct {
		ClientEmail string `json:"client_email"`
		PrivateKey  string `json:"private_key"`
		TokenURI    string `json:"token_uri"`
	}
	_ = json.Unmarshal([]byte(keyText), &key)
	if key.TokenURI == "" {
		key.TokenURI = DefaultTokenURL
	}
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		spreadsheet: spreadsheetID(credentials["spreadsheet_id"]),
		email:       key.ClientEmail,
		privateKey:  key.PrivateKey,
		tokenURL:    key.TokenURI,
		baseURL:     strings.TrimRight(baseURL, "/"),
		client:      &http.Client{Timeout: 60 * time.Second},
		now:         time.Now,
	}
}

// spreadsheetID reads a spreadsheet id from the id itself or from the
// spreadsheet's URL.
func spreadsheetID(value string) string {
	value = strings.TrimSpace(value)
	if index := strings.Index(value, "/d/"); index >= 0 {
		rest := value[index+3:]
		if slash := strings.IndexAny(rest, "/?#"); slash >= 0 {
			rest = rest[:slash]
		}
		return rest
	}
	return value
}

// Objects signs in, which proves the key, and lists every tab.
func (s *Source) Objects() ([]ObjectInfo, error) {
	sheets, err := s.tabs()
	if err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(sheets))
	for _, tab := range sheets {
		out = append(out, ObjectInfo{Name: tab.title, Label: tab.title})
	}
	return out, nil
}

// Describe reads the whole tab, answers the header's columns with their
// samples from the first rows, and counts every filled row.
func (s *Source) Describe(name string) (*Description, error) {
	tab, err := s.tab(name)
	if err != nil {
		return nil, err
	}
	values, err := s.values(tab.title, "")
	if err != nil {
		return nil, err
	}
	columns := header(values)
	rows := []Row{}
	total := 0
	for index := 1; index < len(values); index++ {
		row := flatten(columns, values[index], index+1)
		if row == nil {
			continue
		}
		total++
		if len(rows) < sampleRows {
			rows = append(rows, row)
		}
	}
	return &Description{
		Source:  "sheets",
		Object:  name,
		Label:   tab.title,
		Fields:  contract.Sample(described(columns, rows), rows),
		Rows:    total,
		Bytes:   0,
		Hash:    hashValues(values),
		Counted: true,
	}, nil
}

// Page reads the rows from the cursor's row number. The first page reads
// the whole tab once, so the hash of the values travels with the cursor.
// A later page reads its own range and keeps the hash. Total is the rows
// seen so far.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	tab, err := s.tab(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset := 0
	hash := ""
	start := 2
	if cursor != nil {
		offset = cursor.Offset
		hash = cursor.Hash
		if parsed, err := strconv.Atoi(cursor.Token); err == nil && parsed >= 2 {
			start = parsed
		}
	}
	head, err := s.values(tab.title, "1:1")
	if err != nil {
		return nil, err
	}
	columns := header(head)
	var chunk [][]any
	more := false
	if hash == "" {
		values, err := s.values(tab.title, "")
		if err != nil {
			return nil, err
		}
		hash = hashValues(values)
		if start <= len(values) {
			chunk = values[start-1:]
		}
		if len(chunk) > limit {
			chunk = chunk[:limit]
			more = true
		}
	} else {
		chunk, err = s.values(tab.title, fmt.Sprintf("%d:%d", start, start+limit-1))
		if err != nil {
			return nil, err
		}
		more = len(chunk) == limit
	}
	rows := make([]Row, 0, len(chunk))
	for index, cells := range chunk {
		if row := flatten(columns, cells, start+index); row != nil {
			rows = append(rows, row)
		}
	}
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if more {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(start + len(chunk))}
	}
	return page, nil
}

// Delta reads the whole tab and compares the hash of its values with the
// cursor's.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	tab, err := s.tab(name)
	if err != nil {
		return nil, err
	}
	values, err := s.values(tab.title, "")
	if err != nil {
		return nil, err
	}
	hash := hashValues(values)
	if cursor != nil && cursor.Hash == hash {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: hash}, nil
}

// tabs reads the spreadsheet's sheets once per source.
func (s *Source) tabs() ([]sheet, error) {
	if s.listed {
		return s.sheets, nil
	}
	if err := s.signIn(); err != nil {
		return nil, err
	}
	body, status, err := s.get("/v4/spreadsheets/"+url.PathEscape(s.spreadsheet), url.Values{"fields": {"properties.title,sheets.properties"}})
	if err != nil {
		return nil, err
	}
	if status == http.StatusNotFound {
		return nil, &Failure{Code: "not_connected", Message: fmt.Sprintf("Google has no spreadsheet %s the service account can open. Connect Google Sheets again with the id from the spreadsheet's URL.", s.spreadsheet)}
	}
	if err := s.refusal(status, body); err != nil {
		return nil, err
	}
	var answer struct {
		Sheets []struct {
			Properties struct {
				SheetID any    `json:"sheetId"`
				Title   string `json:"title"`
				Type    string `json:"sheetType"`
			} `json:"properties"`
		} `json:"sheets"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Google's answer does not parse: %s.", err)}
	}
	sheets := []sheet{}
	for _, listed := range answer.Sheets {
		if listed.Properties.Type != "" && listed.Properties.Type != "GRID" {
			continue
		}
		sheets = append(sheets, sheet{id: contract.Scalar(listed.Properties.SheetID), title: listed.Properties.Title})
	}
	s.sheets = sheets
	s.listed = true
	return sheets, nil
}

// tab finds one sheet by title or id.
func (s *Source) tab(name string) (*sheet, error) {
	sheets, err := s.tabs()
	if err != nil {
		return nil, err
	}
	for index := range sheets {
		if sheets[index].title == name || sheets[index].id == name {
			return &sheets[index], nil
		}
	}
	titles := make([]string, 0, len(sheets))
	for _, tab := range sheets {
		titles = append(titles, tab.title)
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("The spreadsheet has no sheet %s. The sheets are %s.", name, strings.Join(titles, ", "))}
}

// values reads one range of a tab as the text the sheet shows. An empty
// rows range reads the whole tab.
func (s *Source) values(title, rows string) ([][]any, error) {
	if err := s.signIn(); err != nil {
		return nil, err
	}
	target := "'" + strings.ReplaceAll(title, "'", "''") + "'"
	if rows != "" {
		target += "!" + rows
	}
	query := url.Values{"majorDimension": {"ROWS"}, "valueRenderOption": {"FORMATTED_VALUE"}, "dateTimeRenderOption": {"FORMATTED_STRING"}}
	body, status, err := s.get("/v4/spreadsheets/"+url.PathEscape(s.spreadsheet)+"/values/"+url.PathEscape(target), query)
	if err != nil {
		return nil, err
	}
	if err := s.refusal(status, body); err != nil {
		return nil, err
	}
	var answer struct {
		Values [][]any `json:"values"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Google's answer does not parse: %s.", err)}
	}
	return answer.Values, nil
}

// column is one header cell as a column.
type column struct {
	name string
}

// header reads the first row as column names: each cell folded to
// snake_case, an empty cell named by its letter, and a repeat suffixed.
func header(values [][]any) []column {
	if len(values) == 0 {
		return nil
	}
	taken := map[string]bool{"row": true}
	columns := make([]column, 0, len(values[0]))
	for index, cell := range values[0] {
		name := columnName(contract.Scalar(cell))
		if name == "" {
			name = "column_" + strings.ToLower(letter(index))
		}
		base := name
		for suffix := 2; taken[name]; suffix++ {
			name = base + "_" + strconv.Itoa(suffix)
		}
		taken[name] = true
		columns = append(columns, column{name: name})
	}
	return columns
}

// letter names a zero-based column index the way the sheet does.
func letter(index int) string {
	name := ""
	for index >= 0 {
		name = string(rune('A'+index%26)) + name
		index = index/26 - 1
	}
	return name
}

// flatten reads one row of cells by the header, with the sheet row number
// as its id. A row with no filled cell reads as nothing.
func flatten(columns []column, cells []any, number int) Row {
	row := Row{"row": strconv.Itoa(number)}
	filled := false
	for index, col := range columns {
		value := ""
		if index < len(cells) {
			value = strings.TrimSpace(contract.Scalar(cells[index]))
		}
		if value != "" {
			filled = true
		}
		row[col.name] = value
	}
	if !filled {
		return nil
	}
	return row
}

// described guesses each column's type from the rows read: a number when
// every filled cell is one, a boolean when every cell is TRUE or FALSE, a
// date when every cell parses as one, an email when every cell carries an
// at sign, else a string.
func described(columns []column, rows []Row) []contract.Column {
	out := []contract.Column{{Name: "row", Guess: "id"}}
	for _, col := range columns {
		out = append(out, contract.Column{Name: col.name, Guess: guess(col.name, rows)})
	}
	return out
}

func guess(name string, rows []Row) string {
	numbers, booleans, dates, emails, filled := 0, 0, 0, 0, 0
	for _, row := range rows {
		value := row[name]
		if value == "" {
			continue
		}
		filled++
		if _, err := strconv.ParseFloat(strings.ReplaceAll(value, ",", ""), 64); err == nil {
			numbers++
		}
		switch strings.ToUpper(value) {
		case "TRUE", "FALSE":
			booleans++
		}
		if isDate(value) {
			dates++
		}
		if strings.Count(value, "@") == 1 && !strings.ContainsAny(value, " ") {
			emails++
		}
	}
	switch {
	case filled == 0:
		return "string"
	case booleans == filled:
		return "boolean"
	case numbers == filled:
		return "number"
	case dates == filled:
		return "date"
	case emails == filled:
		return "email"
	default:
		return "string"
	}
}

func isDate(value string) bool {
	for _, layout := range []string{"2006-01-02", "1/2/2006", "01/02/2006", "2006-01-02 15:04:05", "1/2/2006 15:04:05", time.RFC3339} {
		if _, err := time.Parse(layout, value); err == nil {
			return true
		}
	}
	return false
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a header cell as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// hashValues is the change mark: the SHA-256 of the tab's values as JSON.
func hashValues(values [][]any) string {
	if values == nil {
		values = [][]any{}
	}
	encoded, _ := json.Marshal(values)
	sum := sha256.Sum256(encoded)
	return hex.EncodeToString(sum[:])
}

// signIn signs a JWT with the service account's key and exchanges it for
// an access token once per process.
func (s *Source) signIn() error {
	if s.token != "" {
		return nil
	}
	if s.email == "" || s.privateKey == "" || s.spreadsheet == "" {
		return &Failure{Code: "not_connected", Message: "Connect Google Sheets with a service account key and the spreadsheet id first."}
	}
	assertion, err := s.assertion()
	if err != nil {
		return err
	}
	form := url.Values{"grant_type": {"urn:ietf:params:oauth:grant-type:jwt-bearer"}, "assertion": {assertion}}
	request, err := http.NewRequest(http.MethodPost, s.tokenURL, strings.NewReader(form.Encode()))
	if err != nil {
		return &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	body, status, err := s.do(request)
	if err != nil {
		return err
	}
	if status == http.StatusUnauthorized || status == http.StatusBadRequest || status == http.StatusForbidden {
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Google refused the service account key: %s. Connect Google Sheets again with a key downloaded for an account that still exists.", oauthMessage(body))}
	}
	if status < 200 || status > 299 {
		return &Failure{Code: "source", Message: fmt.Sprintf("Google answered %d at sign-in: %s", status, oauthMessage(body))}
	}
	var granted struct {
		AccessToken string `json:"access_token"`
	}
	if json.Unmarshal(body, &granted) != nil || granted.AccessToken == "" {
		return &Failure{Code: "source", Message: "Google's sign-in answer carried no access token."}
	}
	s.token = granted.AccessToken
	return nil
}

// assertion builds the RS256 JWT the token endpoint accepts: the account
// as issuer, the read-only scope, the token endpoint as audience, and one
// hour of life.
func (s *Source) assertion() (string, error) {
	key, err := parseKey(s.privateKey)
	if err != nil {
		return "", err
	}
	now := s.now().Unix()
	head := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256","typ":"JWT"}`))
	claims, _ := json.Marshal(map[string]any{
		"iss":   s.email,
		"scope": Scope,
		"aud":   s.tokenURL,
		"iat":   now,
		"exp":   now + 3600,
	})
	signing := head + "." + base64.RawURLEncoding.EncodeToString(claims)
	digest := sha256.Sum256([]byte(signing))
	signature, err := rsa.SignPKCS1v15(rand.Reader, key, crypto.SHA256, digest[:])
	if err != nil {
		return "", &Failure{Code: "source", Message: fmt.Sprintf("the service account key did not sign: %s.", err)}
	}
	return signing + "." + base64.RawURLEncoding.EncodeToString(signature), nil
}

// parseKey reads the PEM private key Google writes into the key file, in
// PKCS #8 or PKCS #1 form.
func parseKey(text string) (*rsa.PrivateKey, error) {
	block, _ := pem.Decode([]byte(strings.ReplaceAll(text, `\n`, "\n")))
	if block == nil {
		return nil, &Failure{Code: "not_connected", Message: "The service account key carries no PEM private key. Connect Google Sheets again with the JSON key Google downloads."}
	}
	if key, err := x509.ParsePKCS8PrivateKey(block.Bytes); err == nil {
		if rsaKey, ok := key.(*rsa.PrivateKey); ok {
			return rsaKey, nil
		}
		return nil, &Failure{Code: "not_connected", Message: "The service account key is not an RSA key."}
	}
	if key, err := x509.ParsePKCS1PrivateKey(block.Bytes); err == nil {
		return key, nil
	}
	return nil, &Failure{Code: "not_connected", Message: "The service account key does not parse. Connect Google Sheets again with the JSON key Google downloads."}
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	target := s.baseURL + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	return s.do(request)
}

func (s *Source) do(request *http.Request) ([]byte, int, error) {
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Google did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 64<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Google's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status the Sheets API answers into the one failure the
// runtime reads. A 403 is the spreadsheet not shared with the account.
func (s *Source) refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Google refused the access token. Connect Google Sheets again."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Google refused the service account for the spreadsheet: %s. Share the spreadsheet with %s as a viewer.", apiMessage(body), s.email)}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Google asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Google answered %d: %s", status, apiMessage(body))}
	}
	return nil
}

func apiMessage(body []byte) string {
	var failure struct {
		Error struct {
			Message string `json:"message"`
			Status  string `json:"status"`
		} `json:"error"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error.Message != "" {
		return failure.Error.Message
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

func oauthMessage(body []byte) string {
	var failure struct {
		Error       string `json:"error"`
		Description string `json:"error_description"`
	}
	if json.Unmarshal(body, &failure) == nil && failure.Error != "" {
		if failure.Description != "" {
			return failure.Error + ", " + failure.Description
		}
		return failure.Error
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}
