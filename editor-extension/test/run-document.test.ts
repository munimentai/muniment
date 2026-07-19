import assert from "node:assert/strict";
import test from "node:test";
import { RunDocument } from "../src/run-document";
import type { RunStreamMessage, RunStreamSubscription } from "../src/transport";

class FakeSubscription implements RunStreamSubscription {
  readonly subscriptionId = "subscription-1";
  readonly runId = "run-1";
  readonly firstAvailableRunSeq = 1;
  readonly currentRunSeq = 2;
  readonly window = { maxEvents: 10, maxBytes: 1000 };
  listener?: (message: RunStreamMessage) => void;
  acknowledgements: number[] = [];
  snapshotsAtAck: string[] = [];
  disposed = false;
  content = () => "";
  acknowledgeError: Error | undefined;

  onDidReceiveMessage(listener: (message: RunStreamMessage) => void): { dispose(): void } {
    this.listener = listener;
    return { dispose: () => { this.listener = undefined; } };
  }
  async acknowledge(runSeq: number): Promise<void> {
    this.snapshotsAtAck.push(this.content());
    this.acknowledgements.push(runSeq);
    if (this.acknowledgeError) throw this.acknowledgeError;
  }
  emit(message: RunStreamMessage): void { this.listener?.(message); }
  dispose(): void { this.disposed = true; this.listener = undefined; }
}

const event = (runSeq: number, eventType: string, receipt?: any): RunStreamMessage => ({
  type: "run.event", runSeq, eventType, eventVersion: 1,
  recordedAt: `2026-07-18T12:00:0${runSeq}Z`,
  payload: { withheld: true, ...(receipt ? { receipt } : {}) },
});
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test("renders ordered withheld activity and acknowledges only after incorporation", async () => {
  const subscription = new FakeSubscription();
  const document = new RunDocument("run-1", 1);
  subscription.content = () => document.content;
  await document.attach(Promise.resolve(subscription));

  subscription.emit(event(2, "tool.requested"));
  subscription.emit(event(3, "tool.effect.completed"));
  await settle();

  assert.ok(document.content.indexOf("2 · tool.requested") < document.content.indexOf("3 · tool.effect.completed"));
  assert.match(document.content, /output withheld · connection not granted/);
  assert.deepEqual(subscription.acknowledgements, [2, 3]);
  assert.match(subscription.snapshotsAtAck[0], /2 · tool.requested/);
  document.dispose();
});

test("renders only present receipt fields and terminal status", async () => {
  const subscription = new FakeSubscription();
  const document = new RunDocument("run-1", 1);
  await document.attach(Promise.resolve(subscription));
  subscription.emit(event(2, "run.completed", {
    route: "local", cost: "$0.01", capabilities: [{ name: "search", version: "1" }],
  }));
  await settle();

  assert.match(document.content, /Status:\*\* Completed/);
  assert.match(document.content, /Route: local/);
  assert.match(document.content, /Cost: \$0\.01/);
  assert.match(document.content, /Capability: search @ 1/);
  assert.doesNotMatch(document.content, /Model:|Time:/);
  assert.deepEqual(subscription.acknowledgements, [2]);
  assert.equal(subscription.disposed, true);
});

for (const [eventType, status] of [
  ["run.completed", "Completed"],
  ["run.failed", "Failed · Refresh Threads to retry."],
  ["run.cancelled", "Cancelled"],
] as const) {
  test(`preserves ${eventType} when its acknowledgement fails`, async () => {
    const subscription = new FakeSubscription();
    subscription.acknowledgeError = new Error("raw secret transport failure");
    const document = new RunDocument("run-1", 1);
    await document.attach(Promise.resolve(subscription));
    subscription.emit(event(2, eventType));
    await settle();

    assert.match(document.content, new RegExp(`Status:\\*\\* ${status.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
    assert.match(document.content, /Final delivery acknowledgement failed/);
    assert.doesNotMatch(document.content, /raw secret|Stream interrupted/);
    assert.equal(subscription.disposed, true);
  });
}

for (const [eventType, status] of [
  ["run.completed", "Completed"],
  ["run.failed", "Failed · Refresh Threads to retry."],
  ["run.cancelled", "Cancelled"],
] as const) {
  for (const acknowledgeFails of [false, true]) {
    test(`preserves coalesced ${eventType} through close${acknowledgeFails ? " when acknowledgement fails" : ""}`, async () => {
      const subscription = new FakeSubscription();
      if (acknowledgeFails) subscription.acknowledgeError = new Error("raw secret transport failure");
      const document = new RunDocument("run-1", 1);
      await document.attach(Promise.resolve(subscription));
      subscription.emit(event(2, eventType));
      subscription.emit({ type: "stream.closed", runSeq: 3, code: "hostile-close-code", resumable: true });
      await settle();

      assert.match(document.content, new RegExp(`Status:\\*\\* ${status.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
      assert.doesNotMatch(document.content, /Stream closed|hostile-close-code|raw secret/);
      if (acknowledgeFails) assert.match(document.content, /Final delivery acknowledgement failed/);
      assert.equal(subscription.disposed, true);
    });
  }
}

test("caught-up is silent and closures use sanitized retry instructions", async () => {
  const subscription = new FakeSubscription();
  const document = new RunDocument("run-1", 1);
  await document.attach(Promise.resolve(subscription));
  const before = document.content;
  subscription.emit({ type: "subscription.caught_up", runSeq: 1 });
  await settle();
  assert.equal(document.content, before);

  subscription.emit({ type: "stream.closed", runSeq: 2, code: "secret-capability", resumable: true });
  await settle();
  assert.match(document.content, /Refresh Threads to resume this run/);
  assert.doesNotMatch(document.content, /secret-capability/);
  assert.equal(subscription.disposed, true);
});

test("dispose rejects late subscription and ignores stale events", async () => {
  let resolve!: (subscription: RunStreamSubscription) => void;
  const pending = new Promise<RunStreamSubscription>((value) => { resolve = value; });
  const document = new RunDocument("run-1", 1);
  const attaching = document.attach(pending);
  document.dispose();
  const subscription = new FakeSubscription();
  resolve(subscription);
  await attaching;
  assert.equal(subscription.disposed, true);
  assert.equal(subscription.listener, undefined);
});

test("out-of-order activity closes the stream without rendering hostile fields", async () => {
  const subscription = new FakeSubscription();
  const document = new RunDocument("run-1", 1);
  await document.attach(Promise.resolve(subscription));
  subscription.emit(event(4, "payload-secret"));
  await settle();
  assert.match(document.content, /Stream interrupted/);
  assert.doesNotMatch(document.content, /payload-secret/);
  assert.equal(subscription.disposed, true);
});
