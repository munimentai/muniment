import assert from "node:assert/strict";
import test from "node:test";
import { editorFileContext, editorSelectionContext } from "../src/editor-context";

test("projects a current unsaved file buffer with a normalized Windows path", () => {
  assert.deepEqual(editorFileContext({
    scheme: "file",
    text: "unsaved contents",
    workspaceRelativePath: "packages\\app\\src\\draft.ts",
  }), {
    file_text: "unsaved contents",
    file: "packages/app/src/draft.ts",
  });
});

test("rejects a current file outside an open workspace", () => {
  assert.equal(editorFileContext({ scheme: "file", text: "contents" }), undefined);
});

test("projects an in-workspace selection with a normalized relative file identity", () => {
  assert.deepEqual(editorSelectionContext({
    scheme: "file",
    isEmpty: false,
    selectedText: "const answer = 42;",
    workspaceRelativePath: "packages\\app\\src\\answer.ts",
  }), {
    selected_text: "const answer = 42;",
    selected_file: "packages/app/src/answer.ts",
  });
});

test("preserves the workspace-relative identity supplied for a multi-root workspace", () => {
  assert.deepEqual(editorSelectionContext({
    scheme: "file",
    isEmpty: false,
    selectedText: "selected",
    workspaceRelativePath: "src/second-root.ts",
  }), { selected_text: "selected", selected_file: "src/second-root.ts" });
});

test("omits context without an eligible active workspace selection", () => {
  assert.equal(editorSelectionContext(undefined), undefined, "no active editor");
  assert.equal(editorSelectionContext({
    scheme: "file", isEmpty: true, selectedText: "", workspaceRelativePath: "src/file.ts",
  }), undefined, "empty selection");
  assert.equal(editorSelectionContext({
    scheme: "untitled", isEmpty: false, selectedText: "text", workspaceRelativePath: "file.ts",
  }), undefined, "non-file document");
  assert.equal(editorSelectionContext({
    scheme: "file", isEmpty: false, selectedText: "text",
  }), undefined, "outside all workspaces");
});

test("refuses absolute or escaping file identities", () => {
  for (const workspaceRelativePath of ["/secret.txt", "C:\\secret.txt", "../secret.txt"]) {
    assert.equal(editorSelectionContext({
      scheme: "file", isEmpty: false, selectedText: "text", workspaceRelativePath,
    }), undefined);
  }
});
