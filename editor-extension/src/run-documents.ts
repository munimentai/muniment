import { RunDocument } from "./run-document";
import type { RunStartAccepted, RunStreamSubscription } from "./transport";

/** VS Code-independent ownership and coordination for live run documents. */
export class RunDocumentStore {
  private readonly documents = new Map<string, RunDocument>();

  constructor(private readonly changed: (uri: string) => void = () => undefined) {}

  content(uri: string): string {
    return this.documents.get(uri)?.content ?? "";
  }

  open(uri: string, runId: string, committedSeq: number): RunDocument {
    this.documents.get(uri)?.dispose();
    const document = new RunDocument(runId, committedSeq);
    document.onDidChange(() => this.changed(uri));
    this.documents.set(uri, document);
    this.changed(uri);
    return document;
  }

  clear(): void {
    for (const document of this.documents.values()) document.dispose();
    this.documents.clear();
  }
}

export interface AcceptedRunHost {
  streamRun(runId: string, afterRunSeq: number): Promise<RunStreamSubscription>;
  showDocument(uri: string): Promise<void>;
}

export function acceptedRunUri(runId: string): string {
  return `muniment-run:/${encodeURIComponent(runId)}.md`;
}

export async function openAcceptedRun(
  accepted: RunStartAccepted,
  documents: RunDocumentStore,
  host: AcceptedRunHost,
): Promise<void> {
  const uri = acceptedRunUri(accepted.runId);
  const run = documents.open(uri, accepted.runId, accepted.committedSeq);
  void run.attach(host.streamRun(accepted.runId, accepted.committedSeq));
  await host.showDocument(uri);
}
