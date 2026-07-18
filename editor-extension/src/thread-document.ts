import type { RedactedThreadEntry, ThreadOpenPage } from "./transport";

export const WITHHELD_OUTPUT = "output withheld · connection not granted";

export function formatThreadDocument(title: string, page: ThreadOpenPage): string {
  const entries = [...page.entries].sort((left, right) => left.runSeq - right.runSeq);
  const sections = entries.map((entry) => formatEntry(entry));
  const pagination = page.nextCursor
    ? "Older history is not shown in this preview."
    : undefined;
  return [`# ${title}`, ...sections, pagination].filter((value): value is string => value !== undefined).join("\n\n") + "\n";
}

function formatEntry(entry: RedactedThreadEntry): string {
  const label = entry.kind.trim() || "entry";
  return `## ${label}\n\n${entry.text ?? WITHHELD_OUTPUT}`;
}

export type ThreadDocumentResult =
  | { kind: "ready"; content: string }
  | { kind: "stale" }
  | { kind: "error"; message: string };

/** Coordinates async previews without depending on VS Code or transport details. */
export class ThreadDocumentLoader {
  private generation = 0;
  private disposed = false;

  constructor(private readonly open: (threadId: string) => Promise<ThreadOpenPage>) {}

  async load(threadId: string, title: string): Promise<ThreadDocumentResult> {
    if (this.disposed) return { kind: "stale" };
    const generation = ++this.generation;
    try {
      const page = await this.open(threadId);
      if (this.disposed || generation !== this.generation) return { kind: "stale" };
      return { kind: "ready", content: formatThreadDocument(title, page) };
    } catch {
      if (this.disposed || generation !== this.generation) return { kind: "stale" };
      return {
        kind: "error",
        message: "Couldn’t open this thread. Refresh Threads, then try again.",
      };
    }
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
  }
}
