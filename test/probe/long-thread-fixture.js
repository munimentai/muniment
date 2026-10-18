// A thread of many settled turns for the transcript window checks.
// `long-thread.html?turns=N` sets the count, 5,000 by default.
export const LONG_THREAD_TURNS = 5_000

export function longThreadTurns() {
  const asked = Number(new URLSearchParams(location.search).get('turns'))
  return Number.isInteger(asked) && asked > 0 ? asked : LONG_THREAD_TURNS
}

// Each reply carries a heading, a list and a code block, so opening the thread
// exercises the Markdown renderer the way a real conversation does.
export function longThreadHistory(turns = longThreadTurns()) {
  return Array.from({ length: turns }, (_, index) => ({
    runId: `probe-long-${index}`,
    prompt: `Question ${index + 1}: what changed in batch ${index + 1}?`,
    phase: 'complete',
    text: [
      `### Batch ${index + 1}`,
      '',
      `- Read ${index % 7 + 1} files`,
      `- Updated the summary for turn ${index + 1}`,
      '',
      '```text',
      `batch ${index + 1} ok`,
      '```',
      '',
      `Reply ${index + 1} is done.`,
    ].join('\n'),
    receipt: { time: '1.2s' },
    toolActivity: [],
    resumable: false,
  }))
}
