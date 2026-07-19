import { WITHHELD_OUTPUT } from "./thread-document";
import type { RunReceipt, RunStreamMessage, RunStreamSubscription } from "./transport";

const MAX_HISTORY = 100;

export type RunDocumentListener = (content: string) => void;

/** Owns one ordered run stream and its bounded, read-only document projection. */
export class RunDocument {
  private subscription: RunStreamSubscription | undefined;
  private listenerSubscription: { dispose(): void } | undefined;
  private readonly history: string[] = [];
  private listeners = new Set<RunDocumentListener>();
  private lastRunSeq: number;
  private _content: string;
  private disposed = false;
  private generation = 0;
  private processing = Promise.resolve();
  private finalStatus: string | undefined;
  private deliveryWarning: string | undefined;

  constructor(readonly runId: string, committedSeq: number) {
    this.lastRunSeq = committedSeq;
    this._content = this.render("Connecting…");
  }

  get content(): string { return this._content; }

  onDidChange(listener: RunDocumentListener): { dispose(): void } {
    this.listeners.add(listener);
    return { dispose: () => this.listeners.delete(listener) };
  }

  async attach(stream: Promise<RunStreamSubscription>): Promise<void> {
    const generation = ++this.generation;
    try {
      const subscription = await stream;
      if (this.disposed || generation !== this.generation) {
        subscription.dispose();
        return;
      }
      this.subscription = subscription;
      this.listenerSubscription = subscription.onDidReceiveMessage((message) => {
        this.processing = this.processing.then(() => this.receive(message, generation));
      });
      this.update(this.render("Running"));
    } catch {
      if (!this.disposed && generation === this.generation) {
        this.update(this.render("Stream unavailable · Refresh Threads and try again."));
      }
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.listenerSubscription?.dispose();
    this.subscription?.dispose();
    this.listeners.clear();
  }

  private async receive(message: RunStreamMessage, generation: number): Promise<void> {
    if (this.disposed || generation !== this.generation) return;
    if (message.type === "subscription.caught_up") return;
    if (message.type === "stream.closed") {
      if (message.runSeq < this.lastRunSeq || message.runSeq > this.lastRunSeq + 1) return;
      this.lastRunSeq = Math.max(this.lastRunSeq, message.runSeq);
      const status = message.code === "completed"
        ? "Completed"
        : message.resumable
          ? "Stream closed · Refresh Threads to resume this run."
          : "Stream closed · This run cannot be resumed. Start a new run.";
      this.update(this.render(this.finalStatus ?? status, this.deliveryWarning));
      this.closeSubscription();
      return;
    }
    if (message.runSeq !== this.lastRunSeq + 1) {
      this.update(this.render("Stream interrupted · Refresh Threads to resume this run."));
      this.closeSubscription();
      return;
    }

    if (message.type === "permission.pending") {
      this.lastRunSeq = message.runSeq;
      try {
        await this.subscription?.acknowledge(message.runSeq);
      } catch {
        if (!this.disposed && generation === this.generation) {
          this.update(this.render("Stream interrupted · Refresh Threads to resume this run."));
          this.closeSubscription();
        }
      }
      return;
    }

    this.lastRunSeq = message.runSeq;
    this.history.push(formatEvent(message.runSeq, message.eventType, message.recordedAt, message.payload.receipt));
    if (this.history.length > MAX_HISTORY) this.history.splice(0, this.history.length - MAX_HISTORY);
    const terminal = terminalStatus(message.eventType);
    if (terminal) this.finalStatus = terminal;
    this.update(this.render(terminal ?? "Running"));
    try {
      await this.subscription?.acknowledge(message.runSeq);
    } catch {
      if (!this.disposed && generation === this.generation) {
        this.deliveryWarning = terminal ? "Final delivery acknowledgement failed." : undefined;
        this.update(this.render(terminal ?? "Stream interrupted · Refresh Threads to resume this run.",
          this.deliveryWarning));
        this.closeSubscription();
      }
      return;
    }
    if (terminal && !this.disposed && generation === this.generation) this.closeSubscription();
  }

  private closeSubscription(): void {
    this.listenerSubscription?.dispose();
    this.listenerSubscription = undefined;
    this.subscription?.dispose();
    this.subscription = undefined;
  }

  private render(status: string, deliveryWarning?: string): string {
    const omitted = this.history.length === MAX_HISTORY ? "_Earlier activity omitted._\n\n" : "";
    const events = this.history.length === 0 ? "_Waiting for activity._" : this.history.join("\n\n");
    const warning = deliveryWarning ? `\n\n**Delivery:** ${deliveryWarning}` : "";
    return `# Muniment run\n\n**Status:** ${status}${warning}\n\n${omitted}${events}\n`;
  }

  private update(content: string): void {
    this._content = content;
    for (const listener of this.listeners) listener(content);
  }
}

function formatEvent(runSeq: number, eventType: string, recordedAt: string, receipt?: RunReceipt): string {
  const heading = `## ${runSeq} · ${oneLine(eventType)}`;
  const body = [`${oneLine(recordedAt)}`, WITHHELD_OUTPUT];
  if (receipt) body.push(formatReceipt(receipt));
  return `${heading}\n\n${body.join("\n\n")}`;
}

function formatReceipt(receipt: RunReceipt): string {
  const fields: string[] = [];
  for (const [label, value] of [["Route", receipt.route], ["Model", receipt.model],
    ["Cost", receipt.cost], ["Time", receipt.time]] as const) {
    if (value !== undefined) fields.push(`- ${label}: ${oneLine(value)}`);
  }
  for (const capability of receipt.capabilities) {
    fields.push(`- Capability: ${oneLine(capability.name)} @ ${oneLine(capability.version)}`);
  }
  return fields.length === 0 ? "### Receipt" : `### Receipt\n\n${fields.join("\n")}`;
}

function terminalStatus(eventType: string): string | undefined {
  if (eventType === "run.completed") return "Completed";
  if (eventType === "run.failed") return "Failed · Refresh Threads to retry.";
  if (eventType === "run.cancelled") return "Cancelled";
  return undefined;
}

function oneLine(value: string): string {
  return value.replace(/[\r\n\t]/g, " ").replace(/[\u0000-\u001f\u007f]/g, " ");
}
