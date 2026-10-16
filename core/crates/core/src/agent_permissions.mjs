// Asks the runtime before each tool call whether the thread's permission
// policies and the agent's gate mode allow it. A call they do not settle
// waits for the user's answer at the run's permission gate. The runtime
// decides; this extension only carries the question and the answer.
export default function (pi) {
  const readOnly = (name) =>
    pi.getAllTools?.().find((tool) => tool.name === name)?.annotations?.readOnlyHint === true;
  pi.on("tool_call", async (event, ctx) => {
    const call = { tool: event.toolName, input: event.input ?? {}, read_only: readOnly(event.toolName) };
    let verdict;
    try {
      verdict = JSON.parse(await ctx.ui.editor("muniment:permission-check", JSON.stringify(call)));
    } catch {
      return { block: true, reason: "The permission check failed, so the action did not run." };
    }
    if (verdict?.decision === "allow") return undefined;
    if (verdict?.decision !== "ask") {
      return { block: true, reason: verdict?.reason ?? verdict?.error ?? "A policy denies this action." };
    }
    const choice = await ctx.ui.select(
      `Allow ${event.toolName}? ${verdict.reason ?? ""}`.trim(),
      ["Allow once", "Allow for this thread", "Deny"],
    );
    if (choice === "Allow once") return undefined;
    if (choice === "Allow for this thread") {
      try {
        await ctx.ui.editor("muniment:permission-allow", JSON.stringify(call));
      } catch {
        // The answer still allows this call when the policy cannot be saved.
      }
      return undefined;
    }
    return { block: true, reason: "The user did not allow this action." };
  });
}
