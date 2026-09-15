// The desktop's account sign-in: one command that runs Pi's own OAuth login for
// a provider inside an RPC process the desktop owns, and reports each step as a
// notify line the desktop parses. The credential lands in Pi's auth.json.
export default function (pi) {
  pi.registerCommand("muniment-login", {
    description: "Sign in to a provider account",
    handler: async (args, ctx) => {
      const [provider, type = "oauth"] = String(args ?? "").trim().split(/\s+/)
      const say = (payload) => ctx.ui.notify(JSON.stringify({ muniment: "login", provider, ...payload }), "info")
      const registry = ctx.modelRegistry
      const runtime = registry.runtime ?? registry
      if (typeof runtime.login !== "function") {
        say({ stage: "failed", message: "This harness cannot sign in to an account." })
        return
      }
      try {
        say({ stage: "start" })
        const controller = new AbortController()
        const credential = await runtime.login(provider, type, {
          signal: controller.signal,
          prompt: async (prompt) => {
            if (prompt.type === "select") {
              const labels = prompt.options.map((option) => option.label ?? option.id)
              const chosen = await ctx.ui.select(prompt.message, labels)
              return prompt.options.find((option) => (option.label ?? option.id) === chosen)?.id ?? chosen
            }
            return ctx.ui.input(prompt.message, prompt.placeholder ?? "")
          },
          notify: (event) => say({ stage: "event", event }),
        })
        say({ stage: "done", type: credential?.type ?? "oauth" })
      } catch (error) {
        say({ stage: "failed", message: error instanceof Error ? error.message : String(error) })
      }
    },
  })
}
