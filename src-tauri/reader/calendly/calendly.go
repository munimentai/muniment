// Package calendly reads a Calendly account behind the reader contract:
// three objects, event types, scheduled events and invitees, each flattened
// to text fields a mapping can point at. It uses the v2 REST API over
// net/http alone, with a personal access token as the bearer, and it writes
// nothing back. The current user's uri scopes every list. Backfill walks
// each list through its page token, and the invitees walk runs per
// scheduled event. Delta is the hash of the first page, because Calendly
// cannot filter a list by change time.
package calendly

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"muniment.ai/reader/contract"
)

// DefaultBaseURL is Calendly's API host. A test points the reader
// elsewhere.
const DefaultBaseURL = "https://api.calendly.com"

// PageLimit is the most records one Calendly list call returns.
const PageLimit = 100

// sampleRows is how many records Describe reads for its samples.
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

// Source is one Calendly account reached through one personal access
// token.
type Source struct {
	token  string
	base   string
	client *http.Client
	user   string
}

// New opens a source on a personal access token. An empty base URL is the
// live API.
func New(token, baseURL string) *Source {
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Source{
		token:  token,
		base:   strings.TrimRight(baseURL, "/"),
		client: &http.Client{Timeout: 30 * time.Second},
	}
}

type record = map[string]any

// object is one Calendly list: its path, its sort, and the fields in the
// order Describe lists them. The invitees object has no path of its own,
// because its records come from a walk over the scheduled events.
type object struct {
	name   string
	label  string
	path   string
	sort   string
	fields []field
}

// field names one text column, how the kind schema should type it, and
// how it reads out of a record.
type field struct {
	name  string
	guess string
	read  func(record) string
}

var objects = []object{
	{
		name:  "event_types",
		label: "Event types",
		path:  "/event_types",
		sort:  "name:asc",
		fields: []field{
			{"id", "id", uuidOf("uri")},
			{"uri", "string", text("uri")},
			{"name", "string", text("name")},
			{"slug", "string", text("slug")},
			{"active", "boolean", boolean("active")},
			{"duration", "number", text("duration")},
			{"kind", "string", text("kind")},
			{"pooling_type", "string", text("pooling_type")},
			{"type", "string", text("type")},
			{"scheduling_url", "string", text("scheduling_url")},
			{"description", "string", text("description_plain")},
			{"secret", "boolean", boolean("secret")},
			{"booking_method", "string", text("booking_method")},
			{"color", "string", text("color")},
			{"profile_type", "string", nested("profile", "type")},
			{"profile_name", "string", nested("profile", "name")},
			{"owner_id", "id", nestedUUID("profile", "owner")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name:  "scheduled_events",
		label: "Scheduled events",
		path:  "/scheduled_events",
		sort:  "start_time:asc",
		fields: []field{
			{"id", "id", uuidOf("uri")},
			{"uri", "string", text("uri")},
			{"name", "string", text("name")},
			{"status", "string", text("status")},
			{"start_time", "date-time", when("start_time")},
			{"end_time", "date-time", when("end_time")},
			{"event_type_id", "id", uuidOf("event_type")},
			{"location_type", "string", nested("location", "type")},
			{"location", "string", nested("location", "location")},
			{"join_url", "string", nested("location", "join_url")},
			{"invitees_total", "number", nested("invitees_counter", "total")},
			{"invitees_active", "number", nested("invitees_counter", "active")},
			{"hosts", "string", memberships("user_name")},
			{"host_emails", "string", memberships("user_email")},
			{"host_ids", "string", membershipIDs},
			{"canceled_by", "string", nested("cancellation", "canceled_by")},
			{"cancel_reason", "string", nested("cancellation", "reason")},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
	{
		name:  "invitees",
		label: "Invitees",
		fields: []field{
			{"id", "id", uuidOf("uri")},
			{"uri", "string", text("uri")},
			{"email", "email", text("email")},
			{"email_domain", "domain", emailDomain("email")},
			{"name", "string", text("name")},
			{"first_name", "string", text("first_name")},
			{"last_name", "string", text("last_name")},
			{"status", "string", text("status")},
			{"timezone", "string", text("timezone")},
			{"event_id", "id", uuidOf("event")},
			{"rescheduled", "boolean", boolean("rescheduled")},
			{"no_show", "boolean", present("no_show")},
			{"scheduling_method", "string", text("scheduling_method")},
			{"canceled_by", "string", nested("cancellation", "canceled_by")},
			{"cancel_reason", "string", nested("cancellation", "reason")},
			{"utm_source", "string", nested("tracking", "utm_source")},
			{"utm_medium", "string", nested("tracking", "utm_medium")},
			{"utm_campaign", "string", nested("tracking", "utm_campaign")},
			{"answers", "string", answers},
			{"created", "date-time", when("created_at")},
			{"modified", "date-time", when("updated_at")},
		},
	},
}

// eventFields are the columns an invitee row carries from the scheduled
// event it belongs to.
var eventFields = []field{
	{"event_name", "string", text("name")},
	{"event_start_time", "date-time", when("start_time")},
	{"event_type_id", "id", uuidOf("event_type")},
}

func findObject(name string) (*object, error) {
	for index := range objects {
		if objects[index].name == name {
			return &objects[index], nil
		}
	}
	return nil, &Failure{Code: "unknown_object", Message: fmt.Sprintf("Calendly has no %s to read. The objects are event_types, scheduled_events and invitees.", name)}
}

// Objects reads the current user, which proves the token, and lists the
// three objects.
func (s *Source) Objects() ([]ObjectInfo, error) {
	if _, err := s.me(); err != nil {
		return nil, err
	}
	out := make([]ObjectInfo, 0, len(objects))
	for _, object := range objects {
		out = append(out, ObjectInfo{Name: object.name, Label: object.label})
	}
	return out, nil
}

// Describe reads the first page and answers the fields with their
// samples. A list carries no count, so Rows is the rows read and Counted
// is false.
func (s *Source) Describe(name string) (*Description, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	rows, _, err := s.read(object, "", sampleRows)
	if err != nil {
		return nil, err
	}
	return &Description{
		Source:  "calendly",
		Object:  name,
		Label:   object.label,
		Fields:  contract.Sample(object.columns(), rows),
		Rows:    len(rows),
		Bytes:   0,
		Hash:    hashRows(rows),
		Counted: false,
	}, nil
}

// Page reads one page after the cursor's token. Total is the rows seen so
// far, because a list carries no count. The mark is the hash of the first
// page, which Delta reads again, and later pages carry it forward.
func (s *Source) Page(name string, cursor *Cursor, limit int) (*Page, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	if limit <= 0 || limit > PageLimit {
		limit = PageLimit
	}
	offset, hash, token := 0, "", ""
	if cursor != nil {
		offset, hash, token = cursor.Offset, cursor.Hash, cursor.Token
	}
	rows, next, err := s.read(object, token, limit)
	if err != nil {
		return nil, err
	}
	if token == "" {
		hash = hashRows(rows)
	}
	page := &Page{Rows: rows, Offset: offset, Total: offset + len(rows), Hash: hash, Counted: false}
	if next != "" {
		page.Next = &Cursor{Offset: offset + len(rows), Hash: hash, Token: next}
	}
	return page, nil
}

// Delta reads the first page again and compares its hash with the
// cursor's, because Calendly cannot filter a list by change time.
func (s *Source) Delta(name string, cursor *Cursor) (*Delta, error) {
	object, err := findObject(name)
	if err != nil {
		return nil, err
	}
	rows, _, err := s.read(object, "", PageLimit)
	if err != nil {
		return nil, err
	}
	hash := hashRows(rows)
	if cursor != nil && cursor.Hash == hash {
		return &Delta{State: "unchanged"}, nil
	}
	return &Delta{State: "changed", Hash: hash}, nil
}

// read answers one page of an object after the token and the token of the
// page after it: one list page for event types and scheduled events, and
// one step of the invitee walk.
func (s *Source) read(object *object, token string, limit int) ([]Row, string, error) {
	user, err := s.me()
	if err != nil {
		return nil, "", err
	}
	if object.name == "invitees" {
		return s.invitees(object, user, token, limit)
	}
	query := url.Values{"user": {user}, "count": {strconv.Itoa(limit)}, "sort": {object.sort}}
	if token != "" {
		query.Set("page_token", token)
	}
	items, next, err := s.list(object.path, query)
	if err != nil {
		return nil, "", err
	}
	rows := make([]Row, 0, len(items))
	for _, item := range items {
		rows = append(rows, object.row(item))
	}
	return rows, next, nil
}

// walk is where the invitee walk stands: the page token of the scheduled
// events page it reads, the position in that page, and the page token
// inside that event's invitees. It travels as the cursor's token.
type walk struct {
	Events   string `json:"events,omitempty"`
	At       int    `json:"at,omitempty"`
	Invitees string `json:"invitees,omitempty"`
}

// invitees walks the scheduled events and reads each event's invitees
// until the limit fills or the events end. An event whose counter reads no
// invitees costs no call.
func (s *Source) invitees(object *object, user, token string, limit int) ([]Row, string, error) {
	at := walk{}
	if token != "" {
		if err := json.Unmarshal([]byte(token), &at); err != nil {
			return nil, "", &Failure{Code: "source", Message: "The invitees cursor does not parse. Start the backfill again."}
		}
	}
	rows := []Row{}
	for {
		query := url.Values{"user": {user}, "count": {strconv.Itoa(PageLimit)}, "sort": {"start_time:asc"}}
		if at.Events != "" {
			query.Set("page_token", at.Events)
		}
		events, nextEvents, err := s.list("/scheduled_events", query)
		if err != nil {
			return nil, "", err
		}
		for at.At < len(events) {
			event := events[at.At]
			if at.Invitees == "" && counted(event) == 0 {
				at.At++
				continue
			}
			query := url.Values{"count": {strconv.Itoa(limit - len(rows))}}
			if at.Invitees != "" {
				query.Set("page_token", at.Invitees)
			}
			items, next, err := s.list("/scheduled_events/"+uuid(contract.Scalar(event["uri"]))+"/invitees", query)
			if err != nil {
				return nil, "", err
			}
			for _, item := range items {
				row := object.row(item)
				for _, f := range eventFields {
					row[f.name] = f.read(event)
				}
				rows = append(rows, row)
			}
			at.Invitees = next
			if next == "" {
				at.At++
			}
			if len(rows) >= limit {
				if at.At < len(events) {
					return rows, at.encode(), nil
				}
				if nextEvents == "" {
					return rows, "", nil
				}
				return rows, walk{Events: nextEvents}.encode(), nil
			}
		}
		if nextEvents == "" {
			return rows, "", nil
		}
		at = walk{Events: nextEvents}
	}
}

func (w walk) encode() string {
	encoded, _ := json.Marshal(w)
	return string(encoded)
}

// counted reads how many invitees a scheduled event counts.
func counted(event record) int {
	counter, _ := event["invitees_counter"].(map[string]any)
	total, _ := counter["total"].(float64)
	return int(total)
}

// me reads the current user's uri once per source. It is the scope every
// list asks for.
func (s *Source) me() (string, error) {
	if s.user != "" {
		return s.user, nil
	}
	body, status, err := s.get("/users/me", nil)
	if err != nil {
		return "", err
	}
	if err := refusal(status, body); err != nil {
		return "", err
	}
	var answer struct {
		Resource struct {
			URI string `json:"uri"`
		} `json:"resource"`
	}
	if err := json.Unmarshal(body, &answer); err != nil {
		return "", &Failure{Code: "source", Message: fmt.Sprintf("Calendly's answer does not parse: %s.", err)}
	}
	if answer.Resource.URI == "" {
		return "", &Failure{Code: "source", Message: "Calendly named no current user for the token."}
	}
	s.user = answer.Resource.URI
	return s.user, nil
}

// list fetches one page of a list endpoint and answers its records and
// the next page token, empty when the collection ends.
func (s *Source) list(path string, query url.Values) ([]record, string, error) {
	body, status, err := s.get(path, query)
	if err != nil {
		return nil, "", err
	}
	if err := refusal(status, body); err != nil {
		return nil, "", err
	}
	var listed struct {
		Collection []record `json:"collection"`
		Pagination struct {
			NextPageToken string `json:"next_page_token"`
		} `json:"pagination"`
	}
	if err := json.Unmarshal(body, &listed); err != nil {
		return nil, "", &Failure{Code: "source", Message: fmt.Sprintf("Calendly's answer does not parse: %s.", err)}
	}
	return listed.Collection, listed.Pagination.NextPageToken, nil
}

func (s *Source) get(path string, query url.Values) ([]byte, int, error) {
	target := s.base + path
	if len(query) > 0 {
		target += "?" + query.Encode()
	}
	request, err := http.NewRequest(http.MethodGet, target, nil)
	if err != nil {
		return nil, 0, &Failure{Code: "source", Message: err.Error()}
	}
	request.Header.Set("Authorization", "Bearer "+s.token)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Calendly did not answer: %s.", err)}
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 16<<20))
	if err != nil {
		return nil, 0, &Failure{Code: "unreachable", Message: fmt.Sprintf("Calendly's answer did not read: %s.", err)}
	}
	return body, response.StatusCode, nil
}

// refusal turns a status Calendly answers into the one failure the
// runtime reads.
func refusal(status int, body []byte) error {
	switch {
	case status == http.StatusUnauthorized:
		return &Failure{Code: "not_connected", Message: "Calendly refused the personal access token. Connect Calendly again with a token that works."}
	case status == http.StatusForbidden:
		return &Failure{Code: "not_connected", Message: fmt.Sprintf("Calendly refused the request: %s. Connect Calendly again with a token from the account that owns the events.", calendlyMessage(body))}
	case status == http.StatusTooManyRequests:
		return &Failure{Code: "rate_limited", Message: "Calendly asked the reader to wait. Run again in a minute."}
	case status < 200 || status > 299:
		return &Failure{Code: "source", Message: fmt.Sprintf("Calendly answered %d: %s", status, calendlyMessage(body))}
	}
	return nil
}

func calendlyMessage(body []byte) string {
	var failure struct {
		Title   string `json:"title"`
		Message string `json:"message"`
	}
	if json.Unmarshal(body, &failure) == nil {
		if failure.Message != "" {
			return failure.Message
		}
		if failure.Title != "" {
			return failure.Title
		}
	}
	return contract.Clip(strings.TrimSpace(string(body)), 200)
}

// columns lists the object's fields, then the event columns an invitee
// carries.
func (o *object) columns() []contract.Column {
	columns := make([]contract.Column, 0, len(o.fields)+len(eventFields))
	for _, f := range o.fields {
		columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
	}
	if o.name == "invitees" {
		for _, f := range eventFields {
			columns = append(columns, contract.Column{Name: f.name, Guess: f.guess})
		}
	}
	return columns
}

// row flattens one record to its fields.
func (o *object) row(item record) Row {
	row := Row{}
	for _, f := range o.fields {
		row[f.name] = f.read(item)
	}
	return row
}

// hashRows is the change mark: the SHA-256 of the rows as JSON.
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
// profile.
func nested(key, sub string) func(record) string {
	return func(item record) string {
		inner, _ := item[key].(map[string]any)
		return contract.Scalar(inner[sub])
	}
}

// uuidOf reads the id at the end of a resource uri.
func uuidOf(key string) func(record) string {
	return func(item record) string { return uuid(contract.Scalar(item[key])) }
}

func nestedUUID(key, sub string) func(record) string {
	return func(item record) string {
		inner, _ := item[key].(map[string]any)
		return uuid(contract.Scalar(inner[sub]))
	}
}

// uuid is the last path segment of a Calendly uri, or the text itself
// when it carries no slash.
func uuid(uri string) string {
	uri = strings.TrimRight(strings.TrimSpace(uri), "/")
	if slash := strings.LastIndex(uri, "/"); slash >= 0 {
		return uri[slash+1:]
	}
	return uri
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

// present reads an object that Calendly leaves null when the fact does
// not hold, such as no_show, as a flag.
func present(key string) func(record) string {
	return func(item record) string {
		if _, ok := item[key].(map[string]any); ok {
			return "yes"
		}
		return "no"
	}
}

// memberships joins one value of every host of a scheduled event.
func memberships(sub string) func(record) string {
	return func(item record) string {
		list, _ := item["event_memberships"].([]any)
		parts := []string{}
		for _, part := range list {
			member, _ := part.(map[string]any)
			if value := contract.Scalar(member[sub]); value != "" {
				parts = append(parts, value)
			}
		}
		return strings.Join(parts, ",")
	}
}

func membershipIDs(item record) string {
	list, _ := item["event_memberships"].([]any)
	parts := []string{}
	for _, part := range list {
		member, _ := part.(map[string]any)
		if id := uuid(contract.Scalar(member["user"])); id != "" {
			parts = append(parts, id)
		}
	}
	return strings.Join(parts, ",")
}

// answers joins an invitee's questions and answers as question: answer
// pairs.
func answers(item record) string {
	list, _ := item["questions_and_answers"].([]any)
	parts := []string{}
	for _, part := range list {
		pair, _ := part.(map[string]any)
		question := strings.TrimSpace(contract.Scalar(pair["question"]))
		answer := strings.TrimSpace(contract.Scalar(pair["answer"]))
		if question == "" && answer == "" {
			continue
		}
		parts = append(parts, question+": "+answer)
	}
	return strings.Join(parts, " | ")
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

// formatTime reads Calendly's ISO timestamp as RFC 3339 in UTC.
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
