// The reader sidecar speaks one JSON request on stdin and one JSON answer on
// stdout, then exits. The four calls are the reader contract: objects,
// describe, page and delta. Nothing here touches SQLite, and the secret
// arrives in the environment, never on the command line.
package main

import (
	"encoding/json"

	"muniment.ai/reader/contract"
)

// Source is what every network reader implements.
type Source interface {
	Objects() ([]contract.ObjectInfo, error)
	Describe(object string) (*contract.Description, error)
	Page(object string, cursor *contract.Cursor, limit int) (*contract.Page, error)
	Delta(object string, cursor *contract.Cursor) (*contract.Delta, error)
}

func fail(code, format string, args ...any) *contract.Failure {
	return contract.Fail(code, format, args...)
}

// answer runs one request against a source and returns the JSON body.
func answer(source Source, request contract.Request) ([]byte, error) {
	var body any
	var err error
	switch request.Call {
	case "objects":
		var objects []contract.ObjectInfo
		objects, err = source.Objects()
		body = map[string]any{"objects": objects}
	case "describe":
		var description *contract.Description
		description, err = source.Describe(request.Object)
		body = map[string]any{"description": description}
	case "page":
		var page *contract.Page
		page, err = source.Page(request.Object, request.Cursor, request.Limit)
		body = map[string]any{"page": page}
	case "delta":
		var delta *contract.Delta
		delta, err = source.Delta(request.Object, request.Cursor)
		body = map[string]any{"delta": delta}
	default:
		err = fail("unknown_call", "%q is not objects, describe, page or delta", request.Call)
	}
	if err != nil {
		failure, ok := err.(*contract.Failure)
		if !ok {
			failure = fail("source", "%s", err.Error())
		}
		return json.Marshal(map[string]any{"error": failure})
	}
	return json.Marshal(body)
}
