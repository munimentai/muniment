import { AttachTransportError, MAX_RUN_START_CONTEXT_LENGTH, type AttachConnection, type JsonValue, type RedactedThreadSummary, type RunStartAccepted, type RunStreamSubscription, type ThreadOpenPage } from "./transport";

export const OPEN_THREAD_COMMAND = "muniment.openThread";
export const NEW_RUN_COMMAND = "muniment.newRun";
export const NEW_RUN_WITH_CURRENT_FILE_COMMAND = "muniment.newRunWithCurrentFile";

export type RunSubmissionResult =
  | ({ kind: "accepted" } & RunStartAccepted)
  | { kind: "no-op" }
  | { kind: "busy" }
  | { kind: "unavailable" }
  | { kind: "failed"; message: string };

export type ThreadsState =
  | { kind: "loading" }
  | { kind: "pairing" }
  | { kind: "empty" }
  | { kind: "runtime-unavailable" }
  | { kind: "connection-failed" }
  | { kind: "ready"; threads: ThreadItem[] };

export interface ThreadItem {
  threadId: string;
  title: string;
  description: string;
}

export interface ThreadOpenCommand {
  command: typeof OPEN_THREAD_COMMAND;
  title: string;
  arguments: [threadId: string, title: string];
}

export function threadOpenCommand(item: Pick<ThreadItem, "threadId" | "title">): ThreadOpenCommand {
  return {
    command: OPEN_THREAD_COMMAND,
    title: "Open Thread",
    arguments: [item.threadId, item.title],
  };
}

export type AttachConnector = (
  onPairingPending: () => void,
) => Promise<AttachConnection>;

type StateListener = (state: ThreadsState) => void;

/** Runtime-backed state for the native Threads view, kept independent of VS Code. */
export class ThreadsModel {
  private connection: AttachConnection | undefined;
  private listeners = new Set<StateListener>();
  private generation = 0;
  private disposed = false;
  private submittingRun = false;
  private _state: ThreadsState = { kind: "loading" };

  constructor(
    private readonly connect: AttachConnector,
    private readonly formatUpdatedAt: (updatedAt: string) => string = conciseUpdatedAt,
  ) {}

  get state(): ThreadsState {
    return this._state;
  }

  onDidChange(listener: StateListener): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  async openThread(threadId: string): Promise<ThreadOpenPage> {
    if (this.disposed || !this.connection) {
      throw new AttachTransportError("authorization_expired");
    }
    return this.connection.openThread(threadId);
  }

  async submitRun(text: string | undefined, context?: JsonValue): Promise<RunSubmissionResult> {
    if (text === undefined || text.trim().length === 0) return { kind: "no-op" };
    if (context !== undefined && Buffer.byteLength(JSON.stringify(context), "utf8") > MAX_RUN_START_CONTEXT_LENGTH) {
      return { kind: "failed", message: "The selected editor text is too large to attach (64 KiB maximum)." };
    }
    if (this.submittingRun) return { kind: "busy" };
    if (this.disposed || !this.connection) return { kind: "unavailable" };

    this.submittingRun = true;
    try {
      const accepted = await this.connection.startRun(text, context);
      return { kind: "accepted", ...accepted };
    } catch (error) {
      return { kind: "failed", message: runStartFailureMessage(error) };
    } finally {
      this.submittingRun = false;
    }
  }

  async streamRun(runId: string, afterRunSeq: number): Promise<RunStreamSubscription> {
    if (this.disposed || !this.connection) {
      throw new AttachTransportError("authorization_expired");
    }
    return this.connection.streamRun(runId, afterRunSeq);
  }

  async refresh(): Promise<void> {
    if (this.disposed) return;
    const generation = ++this.generation;
    this.connection?.dispose();
    this.connection = undefined;
    this.update({ kind: "loading" });

    try {
      const connection = await this.connect(() => {
        if (generation === this.generation && !this.disposed) this.update({ kind: "pairing" });
      });
      if (generation !== this.generation || this.disposed) {
        connection.dispose();
        return;
      }
      this.connection = connection;
      const page = await connection.listThreads();
      if (generation !== this.generation || this.disposed) return;
      this.update(page.threads.length === 0
        ? { kind: "empty" }
        : { kind: "ready", threads: page.threads.map((thread) => this.project(thread)) });
    } catch (error) {
      if (generation !== this.generation || this.disposed) return;
      this.connection?.dispose();
      this.connection = undefined;
      this.update(isRuntimeUnavailable(error)
        ? { kind: "runtime-unavailable" }
        : { kind: "connection-failed" });
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.connection?.dispose();
    this.connection = undefined;
    this.listeners.clear();
  }

  private project(thread: RedactedThreadSummary): ThreadItem {
    return {
      threadId: thread.threadId,
      title: thread.title,
      description: this.formatUpdatedAt(thread.updatedAt),
    };
  }

  private update(state: ThreadsState): void {
    this._state = state;
    for (const listener of this.listeners) listener(state);
  }
}

function isRuntimeUnavailable(error: unknown): boolean {
  return error instanceof AttachTransportError &&
    (error.code === "runtime_unavailable" || error.code === "desktop_unavailable");
}

function runStartFailureMessage(error: unknown): string {
  if (error instanceof AttachTransportError && error.code === "authorization_expired") {
    return "Muniment authorization expired. Refresh Threads and try again.";
  }
  if (isRuntimeUnavailable(error)) {
    return "Muniment isn’t available. Open the desktop app and try again.";
  }
  return "Muniment couldn’t start the run. Try again.";
}

export function conciseUpdatedAt(updatedAt: string): string {
  const date = new Date(updatedAt);
  if (!Number.isFinite(date.getTime())) return "Updated recently";
  return `Updated ${new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date)}`;
}
