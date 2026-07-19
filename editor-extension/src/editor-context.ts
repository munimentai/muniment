import type { JsonValue } from "./transport";

export interface ActiveSelection {
  scheme: string;
  isEmpty: boolean;
  selectedText: string;
  workspaceRelativePath?: string;
}

export interface ActiveFile {
  scheme: string;
  text: string;
  workspaceRelativePath?: string;
}

/** Projects an editor selection without depending on the VS Code API. */
export function editorSelectionContext(selection: ActiveSelection | undefined): JsonValue | undefined {
  if (!selection || selection.scheme !== "file" || selection.isEmpty ||
      selection.workspaceRelativePath === undefined) return undefined;

  const selectedFile = normalizeWorkspaceRelativePath(selection.workspaceRelativePath);
  if (selectedFile === undefined) return undefined;
  return { selected_text: selection.selectedText, selected_file: selectedFile };
}

/** Projects a whole editor buffer without depending on the VS Code API. */
export function editorFileContext(file: ActiveFile | undefined): JsonValue | undefined {
  if (!file || file.scheme !== "file" || file.workspaceRelativePath === undefined) return undefined;

  const relativeFile = normalizeWorkspaceRelativePath(file.workspaceRelativePath);
  if (relativeFile === undefined) return undefined;
  return { file_text: file.text, file: relativeFile };
}

function normalizeWorkspaceRelativePath(value: string): string | undefined {
  const normalized = value.replace(/\\/g, "/");
  if (normalized.length === 0 || normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized) ||
      normalized.split("/").includes("..")) return undefined;
  return normalized;
}
