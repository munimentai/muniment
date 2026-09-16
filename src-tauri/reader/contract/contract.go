// Package contract holds the wire shapes of the reader sidecar: the request
// the runtime writes, the cursor it hands back, and the four answers.
package contract

import "fmt"

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
