# Security

## Private reports

Use GitHub private vulnerability reporting on the repository's Security tab.
Choose **Report a vulnerability**. Do not report exploitable details in a public issue.
Include the affected version, platform, reproduction steps and likely impact.
Use synthetic data. Do not send live keys, tokens or private conversations.

Security fixes target the latest stable release. Nightly builds are previews.
Maintainers assess reports privately and coordinate disclosure with the reporter.

## Trust boundaries

Muniment runs tools on the user's computer. Provider requests send the selected
conversation context to the configured provider. MCP servers receive tool arguments.
Installed plugins can execute local code. Install extensions only from sources you trust.
Web content and model output must not grant themselves local permissions.

[THREAT_MODEL.md](THREAT_MODEL.md) defines the runtime boundary.
Reports about credential disclosure, permission bypass, arbitrary code execution,
unsafe archive extraction and updater signature bypass belong in the private channel.

## Release security

Release signing credentials stay outside source control and contributor builds.
The updater verifies signed packages before offering installation.
Private build runners do not execute fork pull requests.
A release requires dependency review, secret scanning and platform checks.
