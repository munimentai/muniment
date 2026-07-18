import type { ExtensionContext } from "vscode";

export function activate(_context: ExtensionContext): void {
  // Activation is deliberately inert until the attach transport is introduced.
}

export function deactivate(): void {
  // Nothing to dispose while activation is inert.
}
