import { isDeepStrictEqual } from 'node:util'

// This demo approves one exact tool call. It grants no file or shell access.
export default function (pi) {
  const allowed = JSON.parse(process.env.MUNIMENT_DEMO_APPROVAL)
  pi.on('tool_call', event => {
    const direct = event.toolName === allowed.tool && isDeepStrictEqual(event.input, allowed.args)
    const script = event.toolName === 'codemode' && event.input.code === allowed.code
    if (!direct && !script) return { block: true, reason: 'The demo did not approve this tool call.' }
  })
}
