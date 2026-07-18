import assert from "node:assert/strict";
import test from "node:test";
import { formatThreadDocument, ThreadDocumentLoader, WITHHELD_OUTPUT } from "../src/thread-document";
import type { ThreadOpenPage } from "../src/transport";

test("formats redacted entries deterministically in ascending run order", () => {
  const page: ThreadOpenPage = {
    threadId: "thread-1",
    entries: [
      { runSeq: 9, kind: "assistant", text: "Safe answer" },
      { runSeq: 2, kind: "user", text: "Safe question" },
      { runSeq: 5, kind: "tool" },
    ],
  };

  assert.equal(formatThreadDocument("A title", page),
    `# A title\n\n## user\n\nSafe question\n\n## tool\n\n${WITHHELD_OUTPUT}\n\n## assistant\n\nSafe answer\n`);
});

test("notes omitted older history without following the first-page cursor", async () => {
  const cursors: Array<string | undefined> = [];
  const loader = new ThreadDocumentLoader(async (threadId) => {
    assert.equal(threadId, "thread-1");
    cursors.push(undefined);
    return { threadId, entries: [], nextCursor: "older" };
  });

  const result = await loader.load("thread-1", "A title");

  assert.deepEqual(cursors, [undefined]);
  assert.equal(result.kind, "ready");
  if (result.kind === "ready") assert.match(result.content, /Older history is not shown/);
});

test("maps failures to bounded retry guidance without transport detail", async () => {
  const loader = new ThreadDocumentLoader(async () => {
    throw new Error("secret server stack and capability");
  });

  assert.deepEqual(await loader.load("thread-1", "Title"), {
    kind: "error",
    message: "Couldn’t open this thread. Refresh Threads, then try again.",
  });
});

test("ignores stale and disposed async results", async () => {
  const resolvers: Array<(page: ThreadOpenPage) => void> = [];
  const loader = new ThreadDocumentLoader(() => new Promise((resolve) => resolvers.push(resolve)));
  const first = loader.load("first", "First");
  const second = loader.load("second", "Second");
  resolvers[0]({ threadId: "first", entries: [] });
  resolvers[1]({ threadId: "second", entries: [] });
  assert.deepEqual(await first, { kind: "stale" });
  assert.equal((await second).kind, "ready");

  const disposed = loader.load("third", "Third");
  loader.dispose();
  resolvers[2]({ threadId: "third", entries: [] });
  assert.deepEqual(await disposed, { kind: "stale" });
  assert.deepEqual(await loader.load("fourth", "Fourth"), { kind: "stale" });
});
