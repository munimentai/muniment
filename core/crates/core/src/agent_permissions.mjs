// Asks the runtime before each tool call whether the global, agent and
// thread rules and the thread's permission mode allow it. A call they do not
// settle waits for the user's answer at the run's permission gate, with the
// title and options the runtime gives. In plan mode the agent submits its
// plan with plan_submit, and the approval picks the mode the thread
// continues in. The runtime decides; this extension only carries the
// question and the answer.
export default function (pi) {
  const readOnly = (name) =>
    pi.getAllTools?.().find((tool) => tool.name === name)?.annotations?.readOnlyHint === true;
  const saves = {
    "Allow for this thread": "muniment:permission-allow",
    "Always allow for this agent": "muniment:permission-always",
    "Always allow": "muniment:permission-always-global",
  };
  // A gate nobody answered in time closes without a choice. That is not a
  // refusal, so the agent may try another way.
  const unanswered =
    "Nobody answered the approval request in time, so this action did not run. Nobody refused it: try another way or carry on with other work.";
  const ask = async (ctx, title, value) => JSON.parse(await ctx.ui.editor(title, JSON.stringify(value)));

  pi.on("before_agent_start", async (event, ctx) => {
    let mode;
    try {
      mode = await ask(ctx, "muniment:permission-mode", {});
    } catch {
      return undefined;
    }
    if (mode?.mode !== "plan" || typeof event?.systemPrompt !== "string") return undefined;
    return {
      systemPrompt: `${event.systemPrompt}\n\nThis thread is in plan mode. Read files and run read-only commands to learn what you need, but change nothing: edits, other commands and other side effects are refused. When the plan is ready, submit it with plan_submit. Approving it switches the thread to the mode the user picks, and you then carry it out.`,
    };
  });

  pi.registerTool({
    name: "plan_submit",
    label: "Submit plan",
    description:
      "In plan mode, submit your finished plan for the user's approval. Approval switches the thread to the mode the user picks, and you then carry out the plan.",
    parameters: {
      type: "object",
      properties: { plan: { type: "string", description: "The plan, in Markdown." } },
      required: ["plan"],
      additionalProperties: false,
    },
    async execute(_id, args, _signal, _update, ctx) {
      const text = (value) => ({ content: [{ type: "text", text: value }] });
      let verdict;
      try {
        verdict = await ask(ctx, "muniment:plan-check", { plan: args?.plan ?? "" });
      } catch {
        throw new Error("The plan could not be submitted.");
      }
      if (verdict?.error) throw new Error(verdict.error);
      if (verdict?.decision !== "ask") return text(verdict?.reason ?? "The plan needs no approval. Carry on.");
      const choice = await ctx.ui.select(verdict.title, verdict.options);
      if (choice === undefined || choice === null) {
        return text("Nobody answered the plan approval in time, so the thread stays in plan mode. Nobody refused it.");
      }
      if (choice === "Keep planning") {
        return text("The user wants you to keep planning. Ask what to change, then submit a revised plan.");
      }
      let approved;
      try {
        approved = await ask(ctx, "muniment:plan-approve", { choice });
      } catch {
        approved = undefined;
      }
      if (!approved?.saved) {
        return text("The plan was approved, but the new mode could not be saved, so the thread stays in plan mode.");
      }
      return text(`The user approved the plan. The thread now runs in ${approved.title} mode. Carry out the plan.`);
    },
  });

  pi.on("tool_call", async (event, ctx) => {
    const call = { tool: event.toolName, input: event.input ?? {}, read_only: readOnly(event.toolName) };
    let verdict;
    try {
      verdict = await ask(ctx, "muniment:permission-check", call);
    } catch {
      return { block: true, reason: "The permission check failed, so the action did not run." };
    }
    if (verdict?.decision === "allow") return undefined;
    if (verdict?.decision !== "ask") {
      return { block: true, reason: verdict?.reason ?? verdict?.error ?? "A policy denies this action." };
    }
    const choice = await ctx.ui.select(
      verdict.title ?? `Allow ${event.toolName}? ${verdict.reason ?? ""}`.trim(),
      Array.isArray(verdict.options) ? verdict.options : ["Allow once", "Allow for this thread", "Deny"],
    );
    if (choice === undefined || choice === null) return { block: true, reason: unanswered };
    if (choice === "Allow once") return undefined;
    if (saves[choice]) {
      try {
        const saved = JSON.parse(await ctx.ui.editor(saves[choice], JSON.stringify(call)));
        if (saved?.saved === false) ctx.ui.notify?.(`This action ran, but the rule was not saved. ${saved.reason ?? ""}`.trim(), "warning");
      } catch {
        // The answer still allows this call when the policy cannot be saved.
      }
      return undefined;
    }
    return { block: true, reason: "The user did not allow this action." };
  });
}
