package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"muniment.ai/reader/contract"
)

type fakeSource struct {
	pages map[string]*contract.Page
}

func (f *fakeSource) Objects() ([]contract.ObjectInfo, error) {
	return []contract.ObjectInfo{{Name: "things", Label: "Things"}}, nil
}

func (f *fakeSource) Describe(object string) (*contract.Description, error) {
	if object != "things" {
		return nil, fail("unknown_object", "no %s", object)
	}
	return &contract.Description{Source: "fake", Object: object, Label: "Things", Fields: []contract.FieldDescription{{Name: "id", Guess: "id", Samples: []string{"t_1"}, Filled: 1}}, Rows: 1, Counted: true}, nil
}

func (f *fakeSource) Page(object string, cursor *contract.Cursor, limit int) (*contract.Page, error) {
	if object == "broken" {
		return nil, errors.New("the wire dropped")
	}
	return f.pages[object], nil
}

func (f *fakeSource) Delta(object string, cursor *contract.Cursor) (*contract.Delta, error) {
	return &contract.Delta{State: "unchanged"}, nil
}

func TestAnswerRoutesTheFourCallsAndShapesErrors(t *testing.T) {
	source := &fakeSource{pages: map[string]*contract.Page{"things": {Rows: []contract.Row{{"id": "t_1"}}, Total: 1, Counted: true}}}
	body, err := answer(source, contract.Request{Call: "objects"})
	if err != nil || !strings.Contains(string(body), `"objects":[{"name":"things"`) {
		t.Fatalf("objects: %s %v", body, err)
	}
	body, _ = answer(source, contract.Request{Call: "describe", Object: "things"})
	if !strings.Contains(string(body), `"description":{"source":"fake"`) {
		t.Fatalf("describe: %s", body)
	}
	body, _ = answer(source, contract.Request{Call: "page", Object: "things", Cursor: &contract.Cursor{Offset: 0}, Limit: 10})
	if !strings.Contains(string(body), `"rows":[{"id":"t_1"}]`) || !strings.Contains(string(body), `"next":null`) {
		t.Fatalf("page: %s", body)
	}
	body, _ = answer(source, contract.Request{Call: "delta", Object: "things", Cursor: &contract.Cursor{}})
	if !strings.Contains(string(body), `"state":"unchanged"`) {
		t.Fatalf("delta: %s", body)
	}
	body, _ = answer(source, contract.Request{Call: "describe", Object: "other"})
	if !strings.Contains(string(body), `"code":"unknown_object"`) {
		t.Fatalf("a typed failure keeps its code: %s", body)
	}
	body, _ = answer(source, contract.Request{Call: "page", Object: "broken"})
	if !strings.Contains(string(body), `"code":"source"`) || !strings.Contains(string(body), "the wire dropped") {
		t.Fatalf("a plain error reads as source: %s", body)
	}
	body, _ = answer(source, contract.Request{Call: "dance"})
	if !strings.Contains(string(body), `"code":"unknown_call"`) {
		t.Fatalf("an unknown call: %s", body)
	}
}

func TestRunReadsOneRequestAndNamesAMissingSecret(t *testing.T) {
	t.Setenv("MUNIMENT_READER_SECRET", "")
	var out bytes.Buffer
	code := run([]string{"stripe"}, strings.NewReader(`{"call":"objects"}`), &out)
	var answer struct {
		Error contract.Failure `json:"error"`
	}
	if err := json.Unmarshal(out.Bytes(), &answer); err != nil || code != 1 || answer.Error.Code != "not_connected" {
		t.Fatalf("missing secret: code %d body %s", code, out.String())
	}
	out.Reset()
	code = run([]string{"quickbooks"}, strings.NewReader(`{"call":"objects"}`), &out)
	if code != 1 || !strings.Contains(out.String(), `"unknown_source"`) {
		t.Fatalf("unknown source: %d %s", code, out.String())
	}
	out.Reset()
	code = run([]string{"stripe"}, strings.NewReader(`not json`), &out)
	if code != 1 || !strings.Contains(out.String(), `"invalid_request"`) {
		t.Fatalf("bad request: %d %s", code, out.String())
	}
	if run(nil, strings.NewReader(""), &out) != 2 {
		t.Fatal("usage exits 2")
	}
}
