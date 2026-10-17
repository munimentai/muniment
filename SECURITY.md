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

The headless hosts in `core/` add network surfaces. The runtime server serves
paired devices over TLS with a pinned certificate and scoped device credentials.
The router server serves agent runs with an admin token and signed run tokens.
The self-update launcher installs only releases signed with the release key.

[THREAT_MODEL.md](THREAT_MODEL.md) defines the runtime boundary and the headless hosts.
Reports about credential disclosure, permission bypass, arbitrary code execution,
unsafe archive extraction and updater signature bypass belong in the private channel.
So do pairing or device scope bypass, run token forgery, and launcher rollback or
downgrade attacks.

## Release security

Release signing credentials stay outside source control and contributor builds.
The updater verifies signed packages before offering installation.
Core releases (`core-v*` tags) sign `SHA256SUMS` with a minisign key. The release
job refuses to tag when the key does not match the public key in source.
Private build runners do not execute fork pull requests.
A release requires dependency review, secret scanning and platform checks.
