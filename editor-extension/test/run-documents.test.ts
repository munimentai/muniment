import assert from "node:assert/strict";
import test from "node:test";
import { acceptedRunUri, openAcceptedRun, RunDocumentStore } from "../src/run-documents";
import type { RunStreamMessage, RunStreamSubscription } from "../src/transport";

class FakeSubscription implements RunStreamSubscription {
  readonly subscriptionId = "subscription-1";
  readonly runId = "accepted-run";
  readonly firstAvailableRunSeq = 8;
  readonly currentRunSeq = 8;
  readonly window = { maxEvents: 10, maxBytes: 1000 };
  listener?: (message: RunStreamMessage) => void;
  disposed = false;

  onDidReceiveMessage(listener: (message: RunStreamMessage) => void): { dispose(): void } {
    this.listener = listener;
    return { dispose: () => { this.listener = undefined; } };
  }
  async acknowledge(): Promise<void> {}
  emit(message: RunStreamMessage): void { this.listener?.(message); }
  dispose(): void { this.disposed = true; this.listener = undefined; }
}

const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test("accepted workflow opens a native run URI and streams from the authoritative cursor", async () => {
  const documents = new RunDocumentStore();
  const subscription = new FakeSubscription();
  const streamCalls: Array<[string, number]> = [];
  const shown: string[] = [];

  await openAcceptedRun({ runId: "accepted-run", committedSeq: 7, acceptedAt: "now" }, documents, {
    streamRun: async (runId, cursor) => {
      streamCalls.push([runId, cursor]);
      return subscription;
    },
    showDocument: async (uri) => { shown.push(uri); },
  });
  await settle();

  assert.deepEqual(streamCalls, [["accepted-run", 7]]);
  assert.deepEqual(shown, ["muniment-run:/accepted-run.md"]);
  assert.match(documents.content(shown[0]), /Status:\*\* Running/);
  documents.clear();
});

test("replacement and refresh dispose subscriptions and suppress stale events", async () => {
  const changed: string[] = [];
  const documents = new RunDocumentStore((uri) => changed.push(uri));
  const uri = acceptedRunUri("accepted-run");
  const first = new FakeSubscription();
  const firstDocument = documents.open(uri, "accepted-run", 7);
  await firstDocument.attach(Promise.resolve(first));

  const replacement = documents.open(uri, "accepted-run", 7);
  assert.equal(first.disposed, true);
  const changesAfterReplacement = changed.length;
  first.emit({ type: "run.event", runSeq: 8, eventType: "stale.secret", eventVersion: 1,
    recordedAt: "now", payload: { withheld: true } });
  await settle();
  assert.equal(changed.length, changesAfterReplacement);
  assert.doesNotMatch(documents.content(uri), /stale\.secret/);

  const second = new FakeSubscription();
  await replacement.attach(Promise.resolve(second));
  documents.clear();
  assert.equal(second.disposed, true);
  assert.equal(documents.content(uri), "");
  const changesAfterRefresh = changed.length;
  second.emit({ type: "subscription.caught_up", runSeq: 7 });
  await settle();
  assert.equal(changed.length, changesAfterRefresh);
});
