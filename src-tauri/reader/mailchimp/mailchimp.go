// Package mailchimp reads a Mailchimp account behind the reader contract:
// every audience is one object named by its list id, its members are the
// records, and one campaigns object lists the sends, each flattened to
// text fields a mapping can point at. It uses the Marketing API 3.0 over
// net/http alone, with the API key as basic auth against the data center
// the key names, and it writes nothing back. Backfill walks each list by
// offset. Delta is one members list filtered to records changed after the
// cursor's mark, and the hash of the first campaigns page, because
// campaigns cannot filter by change time.
package mailchimp

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// PageLimit is the most records one Mailchimp list call returns.
const PageLimit = 1000

// sampleRows is how many records Describe reads for its samples.
const sampleRows = 100

// mergeFieldLimit caps the merge fields one row carries.
const mergeFieldLimit = 200

// campaignsName is the one object that is no audience.
const campaignsName = "campaigns"

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

// Source is one Mailchimp account reached through one API key.
type Source struct {
	key       string
	base      string
	client    *http.Client
	audiences map[string]*audience
	listed    bool
}

// audience is one list as an object: its id, its name, and the merge
// fields it carries as columns once they are read.
type audience struct {
	id     string
	name   string
	merges []mergeField
	read   bool
}

// mergeField is one merge field as a column: the tag the member carries
// it under in merge_fields and the column name.
type mergeField struct {
	tag    string
	column contract.Column
}

// New opens a source on an API key. The key's suffix after the dash names
// the data center that forms the host. A base URL points a test at a fake.
func New(secret, baseURL string) *Source {
	key := contract.Credentials(secret)["token"]
	if baseURL == "" {
		if dash := strings.LastIndex(key, "-"); dash >= 0 && dash < len(key)-1 {
			baseURL = "https://" + key[dash+1:] + ".api.mailchimp.com"
		}
	}
	return &Source{
		key:       key,
		base:      strings.TrimRight(baseURL, "/"),
		client:    &http.Client{Timeout: 30 * time.Second},
		audiences: map[string]*audience{},
	}
}

type record = map[string]any

// field names one text column, how the kind schema should type it, and
// how it reads out of a record.
type field struct {
	name  string
	guess string
	read  func(record) string
}

// memberFields are the core columns every audience carries before its own
// merge fields.
var memberFields = []field{
	{"id", "id", text("id")},
	{"email", "email", text("email_address")},
	{"email_domain", "domain", emailDomain("email_address")},
	{"name", "string", memberName},
	{"first_name", "string", merge("FNAME")},
	{"last_name", "string", merge("LNAME")},
	{"phone", "phone", merge("PHONE")},
	{"status", "string", text("status")},
	{"email_type", "string", text("email_type")},
	{"unsubscribe_reason", "string", text("unsubscribe_reason")},
	{"rating", "number", text("member_rating")},
	{"language", "string", text("language")},
	{"vip", "boolean", boolean("vip")},
	{"source", "string", text("source")},
	{"country", "string", nested("location", "country_code")},
	{"region", "string", nested("location", "region")},
	{"timezone", "string", nested("location", "timezone")},
	{"tags", "string", tagNames},
	{"tag_ids", "string", tagIDs},
	{"interests", "string", interests},
	{"open_rate", "number", nested("stats", "avg_open_rate")},
	{"click_rate", "number", nested("stats", "avg_click_rate")},
	{"signed_up", "date-time", when("timestamp_signup")},
	{"opted_in", "date-time", when("timestamp_opt")},
	{"list_id", "id", text("list_id")},
	{"web_id", "id", text("web_id")},
	{"contact_id", "id", text("contact_id")},
	{"modified", "date-time", when("last_changed")},
}

// coreTags are the merge fields the core columns read, so no extra column
// repeats them.
var coreTags = map[string]bool{"FNAME": true, "LNAME": true, "PHONE": true}

var campaignFields = []field{
	{"id", "id", text("id")},
	{"web_id", "id", text("web_id")},
	{"type", "string", text("type")},
	{"status", "string", text("status")},
	{"title", "string", nested("settings", "title")},
	{"subject", "string", nested("settings", "subject_line")},
	{"preview_text", "string", nested("settings", "preview_text")},
	{"from_name", "string", nested("settings", "from_name")},
	{"reply_to", "email", nested("settings", "reply_to")},
	{"list_id", "id", nested("recipients", "list_id")},
	{"list_name", "string", nested("recipients", "list_name")},
	{"recipients", "number", nested("recipients", "recipient_count")},
	{"segment", "string", nested("recipients", "segment_text")},
	{"emails_sent", "number", text("emails_sent")},
	{"opens", "number", nested("report_summary", "opens")},
	{"unique_opens", "number", nested("report_summary", "unique_opens")},
	{"open_rate", "number", nested("report_summary", "open_rate")},
	{"clicks", "number", nested("report_summary", "clicks")},
	{"click_rate", "number", nested("report_summary", "click_rate")},
	{"archive_url", "string", text("archive_url")},
	{"content_type", "string", text("content_type")},
	{"created", "date-time", when("create_time")},
	{"sent", "date-time", when("send_time")},
}

// Objects walks the audiences, which proves the key, and lists each one by
// its id and name, then the campaigns.
func (s *Source) Objects() ([]ObjectInfo, error) {
	listed, err := s.readAudiences()
	if err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(listed)+1)
	for _, a := range listed {
		out = append(out, ObjectInfo{Name: a.id, Label: a.name})
	}
	out = append(out, ObjectInfo{Name: campaignsName, Label: "Campaigns"})
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. The list names its total, so Counted is true.
func (s *Source) Describe(name string) (*Description, error) {
	a, err := s.audience(name)
	if err != nil {
		return nil, err
	}
	items, total, err := s.list(s.path(a), s.query(a, sampleRows, 0))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, s.row(a, item))
	}
	if total < len(rows) {
		total = len(rows)
	}
	hash := hashRows(rows)
	if a != nil {
		hash = newestChanged(items, "")
	}
	return &Description{
		Source:  "mailchimp",
		Object:  name,
		Label:   s.label(a),
		Fields:  contract.Sample(s.columns(a), rows),
		Rows:    total,
		Bytes:   0,
		Hash:    hash,
		Counted: true,
	}, nil
}

// Page reads one page from the cursor's offset token. Total is the list's
// own count. The members mark is the newest change seen, and the campaigns
// mark is the hash of the first page carried forward.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	a, err := s.audience(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, from := 0, "", 0
	if cursor != nil {
		offset, hash = cursor.Offset, cursor.Hash
		if parsed, err := strconv.Atoi(cursor.Token); err == nil && parsed > 0 {
			from = parsed
		}
	}
	items, total, err := s.list(s.path(a), s.query(a, limit, from))
	if err != nil {
		return nil, err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, s.row(a, item))
	}
	if a != nil {
		hash = newestChanged(items, hash)
	} else if from == 0 {
		hash = hashRows(rows)
	}
	if total < offset+len(rows) {
		total = offset + len(rows)
	}
	answer := &Page{Rows: rows, Offset: offset, Total: total, Hash: hash, Counted: true}
	if from+len(rows) < total && len(rows) > 0 {
		answer.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: strconv.Itoa(from + len(rows))}
	}
	return answer, nil
}

// Delta asks an audience for the one member changed after the mark,
// newest first, and reads the first campaigns page again to compare its
// hash with the cursor's.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	a, err := s.audience(name)
	if err != nil {
		return nil, err
	}
	mark := ""
	if cursor != nil {
		mark = cursor.Hash
	}
	if a == nil {
		items, _, err := s.list(s.path(nil), s.query(nil, PageLimit, 0))
		if err != nil {
			return nil, err
		}
		rows := make([]Row, 0, len(items))
		for _, item := range items {
			rows = append(rows, s.row(nil, item))
		}
		hash := hashRows(rows)
		if hash == mark {
			return &Delta{State: "unchanged"}, nil
		}
		return &Delta{State: "changed", Hash: hash}, nil
	}
	query := s.query(a, 1, 0)
	if mark != "" {
		query.Set("since_last_changed", mark)
	}
	items, _, err := s.list(s.path(a), query)
	if err != nil {
		return nil, err
	}
	if len(items) == 0 {
		if mark == "" {
			return &Delta{State: "changed", Hash: ""}, nil
		}
		return &Delta{State: "unchanged"}, nil
	}
	newest := newestChanged(items, mark)
	if newest == mark {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: newest}, nil
}

// audience finds the object by name: nil for the campaigns, else the list
// from the walk Objects ran, else one read of the list itself.
func (s *Source) audience(name string) (*audience, error) {
	if name == campaignsName {
		return nil, nil
	}
	if cached, ok := s.audiences[name]; ok {
		return cached, nil
	}
	body, status, err := s.get("/3.0/lists/"+url.PathEscape(name), url.Values{"fields": {"id,name"}})
	if err != nil {
		return nil, err
	}
	if status == http.StatusNotFound {
		return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Mailchimp has no audience %s to read. The objects are each audience by its list id and campaigns.", name)}
	}
	if err := refusal(status, body); err != nil {
		return nil, err
	}
	var listed struct {
		ID   string `json:"id"`
		Name string `json:"name"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, &Failure{Code: "source", Message: fmt.Sprintf("Mailchimp's answer does not parse: %s.", err)}
	}
	a := &audience{id: listed.ID, name: listed.Name}
	if a.id == "" {
		a.id = name
	}
	s.audiences[a.id] = a
	return a, nil
}

// readAudiences walks every list in the account once per source and
// answers them sorted by name.
func (s *Source) readAudiences() ([]*audience, error) {
	if !s.listed {
		for from := 0; ; {
			body, status, err := s.get("/3.0/lists", url.Values{"count": {strconv.Itoa(PageLimit)}, "offset": {strconv.Itoa(from)}, "fields": {"lists.id,lists.name,total_items"}})
			if err != nil {
				return nil, err
			}
			if err := refusal(status, body); err != nil {
				return nil, err
			}
			var listed struct {
				Lists []struct {
					ID   string `json:"id"`
					Name string `json:"name"`
				} `json:"lists"`
				TotalItems int `json:"total_items"`
			}
			if err := json.Unmarshal(body, &listed); err != nil {
				return nil, &Failure{Code: "source", Message: fmt.Sprintf("Mailchimp's answer does not parse: %s.", err)}
			}
			for _, l := range listed.Lists {
				if _, ok := s.audiences[l.ID]; !ok {
					s.audiences[l.ID] = &audience{id: l.ID, name: l.Name}
				}
			}
			from += len(listed.Lists)
			if len(listed.Lists) == 0 || from >= listed.TotalItems {
				break
			}
		}
		s.listed = true
	}
	out := make([]*audience, 0, len(s.audiences))
	for _, a := range s.audiences {
		out = append(out, a)
	}
	sort.Slice(out, func(x, y int) bool {
		if out[x].name != out[y].name {
			return out[x].name < out[y].name
		}
		return out[x].id < out[y].id
	})
	return out, nil
}

// mergeFields reads an audience's merge field definitions once and keeps
// every one the core columns leave as a column named as the account named
// it. A key that cannot read them reads as having none, because the core
// columns still land.
func (s *Source) mergeFields(a *audience) []mergeField {
	if a.read {
		return a.merges
	}
	a.read = true
	body, status, err := s.get("/3.0/lists/"+url.PathEscape(a.id)+"/merge-fields", url.Values{"count": {"1000"}, "fields": {"merge_fields.tag,merge_fields.name,merge_fields.type"}})
	if err != nil || status != http.StatusOK {
		return nil
	}
	var answer struct {
		MergeFields []struct {
			Tag  string `json:"tag"`
			Name string `json:"name"`
			Type string `json:"type"`
		} `json:"merge_fields"`
	}
	if json.Unmarshal(body, &answer) != nil {
		return nil
	}
	taken := map[string]bool{}
	for _, f := range memberFields {
		taken[f.name] = true
	}
	for _, definition := range answer.MergeFields {
		if coreTags[definition.Tag] {
			continue
		}
		name := columnName(definition.Name)
		if name == "" || taken[name] {
			continue
		}
		taken[name] = true
		a.merges = append(a.merges, mergeField{tag: definition.Tag, column: contract.Column{Name: name, Guess: guessType(definition.Type)}})
	}
	sort.Slice(a.merges, func(x, y int) bool { return a.merges[x].column.Name < a.merges[y].column.Name })
	if len(a.merges) > mergeFieldLimit {
		a.merges = a.merges[:mergeFieldLimit]
	}
	return a.merges
}

func guessType(kind string) string {
	switch kind {
	case "number":
		return "number"
	case "date":
		return "date"
	case "phone":
		return "phone"
	default:
		return "string"
	}
}

// path is the list endpoint of an object.
func (s *Source) path(a *audience) string {
	if a == nil {
		return "/3.0/campaigns"
	}
	return "/3.0/lists/" + url.PathEscape(a.id) + "/members"
}

// query builds one offset page, sorted so the newest change or the newest
// campaign comes first, without the link noise.
func (s *Source) query(a *audience, limit, from int) url.Values {
	query := url.Values{"count": {strconv.Itoa(limit)}, "offset": {strconv.Itoa(from)}}
	if a == nil {
		query.Set("sort_field", "create_time")
		query.Set("sort_dir", "DESC")
		query.Set("exclude_fields", "campaigns._links")
		return query
	}
	query.Set("sort_field", "last_changed")
	query.Set("sort_dir", "DESC")
	query.Set("exclude_fields", "members._links")
	return query
}

func (s *Source) label(a *audience) string {
	if a == nil {
		return "Campaigns"
	}
	return a.name
}

// columns lists an object's fields, then the audience's merge fields.
func (s *Source) columns(a *audience) []contract.Column {
	fields := campaignFields
	if a != nil {
		fields = memberFields
	}
	columns := make([]contract.Column, 0, len(fields))
	for _, f := range fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	if a != nil {
		for _, extra := range s.mergeFields(a) {
			columns = append(columns, extra.column)
		}
	}
	return columns
}

// row flattens one record: the core fields, then every merge field as its
// own column named as the account named it.
func (s *Source) row(a *audience, item record) Row {
	row := Row{}
	if a == nil {
		for _, f := range campaignFields {
			row[f.name] = f.read(item)
		}
		return row
	}
	for _, f := range memberFields {
		row[f.name] = f.read(item)
	}
	values, _ := item["merge_fields"].(map[string]any)
	for _, extra := range s.mergeFields(a) {
		row[extra.column.Name] = contract.Scalar(values[extra.tag])
	}
	return row
}

// list fetches one page of a list endpoint and answers its records and
// the total the answer names.
func (s *Source) list(path string, query url.Values) ([]record, int, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, 0, err
	}
	if err := refusal(status, body); err != nil {
		return nil, 0, err
	}
	var listed struct {
		Members    []record `json:"members"`
		Campaigns  []record `json:"campaigns"`
		TotalItems int      `json:"total_items"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, 0, &Failure{Code: "source", Message: fmt.Sprintf("Mailchimp's answer does not parse: %s.", err)}
	}
	if listed.Members != nil {
		return listed.Members, listed.TotalItems, nil
	}
	return listed.Campaigns, listed.TotalItems, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	if s.base == "" {
		return nil, 0, &Failure{Code: "not_connected", Message: "The Mailchimp API key names no data center. Connect Mailchimp again with a key that ends in its data center, such as -us21."}
	}
	target := s.base + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.SetBasicAuth("muniment", s.key)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Mailchimp did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Mailchimp's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Mailchimp answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Mailchimp refused the API key. Connect Mailchimp again with a key that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Mailchimp refused the request: %s. Connect Mailchimp again with a key from an account that reads audiences and campaigns.", mailchimpMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Mailchimp asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Mailchimp answered %d: %s", status, mailchimpMessage(body))}
	}
	return nil
}

func mailchimpMessage(body []byte) string {
	var failure struct {
		Title  string `json:"title"`
		Detail string `json:"detail"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if failure.Detail != "" {
			return failure.Detail
		}
		if failure.Title != "" {
			return failure.Title
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

var nonWord = regexp.MustCompile(`[^a-z0-9]+`)

// columnName reads a merge field's shown name as one snake_case column.
func columnName(name string) string {
	return strings.Trim(nonWord.ReplaceAllString(strings.ToLower(name), "_"), "_")
}

// hashRows is the change mark for campaigns: the SHA-256 of the rows as
// JSON.
func hashRows(rows []Row) string {
	if rows == nil {
		rows = []Row{}
	}
	encoded, _ := json.Marshal(rows)
	sum := sha256.Sum256(encoded)
	return hex.EncodeToString(sum[:])
}

func text(key string) func(record) string {
	return func(item record) string { return contract.Scalar(item[key]) }
}

// nested reads one value out of an embedded object such as location or
// settings.
func nested(key, sub string) func(record) string {
	return func(item record) string {
		inner, _ := item[key].(map[string]any)
		return contract.Scalar(inner[sub])
	}
}

// merge reads one merge field by its tag.
func merge(tag string) func(record) string {
	return func(item record) string {
		values, _ := item["merge_fields"].(map[string]any)
		return strings.TrimSpace(contract.Scalar(values[tag]))
	}
}

func boolean(key string) func(record) string {
	return func(item record) string {
		switch value := item[key].(type) {
		case bool:
			if value {
				return "yes"
			}
			return "no"
		default:
			return ""
		}
	}
}

func memberName(item record) string {
	if name := strings.TrimSpace(contract.Scalar(item["full_name"])); name != "" {
		return name
	}
	name := strings.TrimSpace(merge("FNAME")(item) + " " + merge("LNAME")(item))
	if name != "" {
		return name
	}
	return contract.Scalar(item["email_address"])
}

// tagNames joins a member's tags by name, sorted.
func tagNames(item record) string {
	list, _ := item["tags"].([]any)
	names := []string{}
	for _, part := range list {
		tag, _ := part.(map[string]any)
		if name := contract.Scalar(tag["name"]); name != "" {
			names = append(names, name)
		}
	}
	sort.Strings(names)
	return strings.Join(names, ",")
}

func tagIDs(item record) string {
	list, _ := item["tags"].([]any)
	ids := []string{}
	for _, part := range list {
		tag, _ := part.(map[string]any)
		if id := contract.Scalar(tag["id"]); id != "" {
			ids = append(ids, id)
		}
	}
	sort.Strings(ids)
	return strings.Join(ids, ",")
}

// interests joins the ids of the interests a member holds, sorted.
func interests(item record) string {
	held, _ := item["interests"].(map[string]any)
	ids := []string{}
	for id, value := range held {
		if on, ok := value.(bool); ok && on {
			ids = append(ids, id)
		}
	}
	sort.Strings(ids)
	return strings.Join(ids, ",")
}

func emailDomain(key string) func(record) string {
	return func(item record) string {
		email := contract.Scalar(item[key])
		if at := strings.LastIndex(email, "@"); at >= 0 && at < len(email)-1 {
			return strings.ToLower(email[at+1:])
		}
		return ""
	}
}

func when(key string) func(record) string {
	return func(item record) string { return formatTime(contract.Scalar(item[key])) }
}

// formatTime reads Mailchimp's ISO timestamp as RFC 3339 in UTC.
func formatTime(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if parsed, err := time.Parse(time.RFC3339Nano, value); err == nil {
		return parsed.UTC().Format(time.RFC3339)
	}
	return value
}

// newestChanged is the members mark: the latest last_changed in a list as
// RFC 3339 in UTC, so the since_last_changed filter reads it back, or the
// previous mark when nothing newer appears.
func newestChanged(items []record, previous string) string {
	var newest time.Time
	for _, item := range items {
		if parsed, err := time.Parse(time.RFC3339Nano, contract.Scalar(item["last_changed"])); err == nil && parsed.After(newest) {
			newest = parsed
		}
	}
	if newest.IsZero() {
		return previous
	}
	if before, err := time.Parse(time.RFC3339Nano, previous); err == nil && before.After(newest) {
		return previous
	}
	return newest.UTC().Format(time.RFC3339)
}
