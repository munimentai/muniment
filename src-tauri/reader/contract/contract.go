// Package contract holds the wire shapes of the reader sidecar: the request
// the runtime writes, the cursor it hands back, and the four answers.
package contract

import (
	"encoding/json"
	"fmt"
	"sort"
	"strconv"
	"strings"
)

// Request is what the runtime writes on stdin.
type Request struct {
	Call   string  `json:"call"`
	Object string  `json:"object,omitempty"`
	Cursor *Cursor `json:"cursor,omitempty"`
	Limit  int     `json:"limit,omitempty"`
}

// Cursor is where a run stands in an object. The runtime stores it in the
// mapping and hands it back. Offset counts rows landed, Hash is the object's
// change mark, and Token is the source's own page token.
type Cursor struct {
	Offset int    `json:"offset"`
	Hash   string `json:"hash"`
	Token  string `json:"token,omitempty"`
}

type ObjectInfo struct {
	Name  string `json:"name"`
	Label string `json:"label"`
}

type FieldDescription struct {
	Name    string   `json:"name"`
	Guess   string   `json:"guess"`
	Samples []string `json:"samples"`
	Filled  int      `json:"filled"`
}

type Description struct {
	Source  string             `json:"source"`
	Object  string             `json:"object"`
	Label   string             `json:"label"`
	Fields  []FieldDescription `json:"fields"`
	Rows    int                `json:"rows"`
	Bytes   int                `json:"bytes"`
	Hash    string             `json:"hash"`
	Counted bool               `json:"counted"`
}

// Row is one record as text by field name.
type Row map[string]string

type Page struct {
	Rows    []Row   `json:"rows"`
	Offset  int     `json:"offset"`
	Total   int     `json:"total"`
	Hash    string  `json:"hash"`
	Next    *Cursor `json:"next"`
	Counted bool    `json:"counted"`
}

type Delta struct {
	State string `json:"state"`
	Hash  string `json:"hash,omitempty"`
}

// Failure is the one error shape the runtime reads.
type Failure struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func (f *Failure) Error() string { return f.Message }

// Fail builds one typed failure.
func Fail(code, format string, args ...any) *Failure {
	return &Failure{Code: code, Message: fmt.Sprintf(format, args...)}
}

// Credentials reads the secret the runtime holds for a source. A source
// that connects with one value gets it under `token`. A source that needs
// several stores them as one JSON object the panel packs, keyed by name.
func Credentials(secret string) map[string]string {
	secret = strings.TrimSpace(secret)
	if strings.HasPrefix(secret, "{") {
		var packed map[string]string
		if json.Unmarshal([]byte(secret), &packed) == nil {
			for key, value := range packed {
				packed[key] = strings.TrimSpace(value)
			}
			return packed
		}
	}
	return map[string]string{"token": secret}
}

// Column names one text column and how the kind schema should type it.
type Column struct {
	Name  string
	Guess string
}

// Sample describes columns from the rows read: three distinct samples each
// and the count of rows that fill it.
func Sample(columns []Column, rows []Row) []FieldDescription {
	fields := make([]FieldDescription, 0, len(columns))
	for _, column := range columns {
		samples := []string{}
		seen := map[string]bool{}
		filled := 0
		for _, row := range rows {
			value := row[column.Name]
			if value == "" {
				continue
			}
			filled++
			if len(samples) < 3 && !seen[value] {
				seen[value] = true
				samples = append(samples, Clip(value, 80))
			}
		}
		fields = append(fields, FieldDescription{Name: column.Name, Guess: column.Guess, Samples: samples, Filled: filled})
	}
	return fields
}

// Scalar reads one JSON value as the text a row carries.
func Scalar(value any) string {
	switch typed := value.(type) {
	case nil:
		return ""
	case string:
		return typed
	case bool:
		if typed {
			return "yes"
		}
		return "no"
	case float64:
		if typed == float64(int64(typed)) {
			return strconv.FormatInt(int64(typed), 10)
		}
		return strconv.FormatFloat(typed, 'f', -1, 64)
	default:
		encoded, err := json.Marshal(typed)
		if err != nil {
			return ""
		}
		return string(encoded)
	}
}

// Joined reads a JSON list as its scalars joined by commas, sorted when
// asked, so a tag list reads the same whatever order the source used.
func Joined(value any, sorted bool) string {
	list, ok := value.([]any)
	if !ok {
		return Scalar(value)
	}
	parts := []string{}
	for _, part := range list {
		if text := Scalar(part); text != "" {
			parts = append(parts, text)
		}
	}
	if sorted {
		sort.Strings(parts)
	}
	return strings.Join(parts, ",")
}

// Clip cuts text to at most max runes.
func Clip(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max])
}
