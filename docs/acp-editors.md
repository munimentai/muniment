# Configure the Muniment ACP adapter

The Linux desktop package ships `muniment-acp` at
`/usr/lib/muniment/muniment-acp`. Muniment does not ship the adapter on Windows
or macOS today. See [ADR 0022's distribution amendment](decisions/0022-acp-agent-interop.md#amendment--2026-08-03-adapter-distribution-and-editor-configuration)
for this contract.

Before you connect an editor, start the Muniment desktop and sign in. The first
connection shows one approval prompt in the desktop. The adapter stores its
credential under `$XDG_CONFIG_HOME/muniment/`, so later launches reconnect
without another prompt.

## Revoked approval

When Muniment revokes a program, the adapter drops its stored credential. The
program needs a fresh visible approval before it can connect again.

## Zed

Add a custom agent to Zed's `agent_servers` setting:

```json
{
  "agent_servers": {
    "Muniment": {
      "type": "custom",
      "command": "/usr/lib/muniment/muniment-acp",
      "args": []
    }
  }
}
```

## JetBrains IDEs

Add the agent to `~/.jetbrains/acp.json`:

```json
{
  "agent_servers": {
    "Muniment": {
      "command": "/usr/lib/muniment/muniment-acp",
      "args": []
    }
  }
}
```

Editor validation remains pending for the current generally available Zed and
JetBrains releases. Muniment makes no compatibility or release-validation claim
for other ACP editors.
