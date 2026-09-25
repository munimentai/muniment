# Installed subscription checks

`runner/subscriptions.mjs` tests the signed installed app without WebDriver or a replacement executable.
The app runs a fixed diagnostic in its main webview.
The diagnostic sends four synthetic prompts in one thread and selects each later model through the composer picker.
Later prompts omit the random token from the first prompt.
Every reply must repeat that token without tools.

The subscription transport records the provider's model field, not the selected route label.
A missing, conflicting, or different model ID fails the check.
Aliases do not waive this check. Select exact model IDs that the subscription supports.
The app also reports its compiled source SHA and whether its build includes WebDriver.

## Native prerequisites

Use a disposable GUI login or a disposable VM on each platform:

- Linux x64 needs the signed AppImage, a display, `xdotool`, and ImageMagick `import`.
- Windows x64 needs the installed signed per-user MSI, PowerShell, and an interactive desktop.
- macOS ARM64 needs the installed signed ARM64 app, its signed update archive, Xcode command-line tools, and Screen Recording permission.
- macOS x64 needs the installed signed x64 app and the same macOS tools and permission.

Node must support the repository's ESM scripts.
The checkout must match the candidate source.
Install the candidate through the native release pipeline before this probe.
Run this probe before any WDIO step replaces the installed executable or DLL.
The existing Ollama and source-built WDIO checks remain separate.

The runner verifies the updater signature against the repository's public key.
It compares the installed payload with the signed package before and after the probe.
It also checks the package digest, platform, asset name, and compiled source SHA.
The runner does not install, uninstall, publish, or promote a package.
Dispose of the native login after the check so its runtime service cannot outlive the test environment.

## Factory subscriptions

The factory's refresh owner supplies one fresh access lease per selected provider.
The lease file is a JSON array with these fields:

```json
[
  {
    "provider": "openai-codex",
    "access": "<fresh access token>",
    "expires_ms": 0,
    "account_id": "<subscription account ID>"
  }
]
```

Replace the example expiry with a Unix millisecond value at least 20 minutes ahead.
Supported provider IDs are `openai-codex`, `anthropic`, `xai`, and `kimi`.
Codex requires `account_id`. Other providers may omit it.
Do not supply refresh tokens, API keys, provider home directories, or `gh` credentials.
The runner rejects extra lease fields and never writes to the lease file.
It copies only the leased access fields into a disposable app profile.
The factory retains the shared credentials and refresh ownership.

## Run a platform check

The native release job supplies a candidate manifest from its pinned release asset metadata:

```json
{
  "source_sha": "<40 lowercase hex characters>",
  "platform": "linux",
  "asset": "nightly-<source SHA>-linux-muniment_<version>_amd64.AppImage",
  "sha256": "<64 lowercase hex characters>",
  "models": [
    { "family": "openai", "id": "<exact supported model ID 1>" },
    { "family": "openai", "id": "<exact supported model ID 2>" },
    { "family": "anthropic", "id": "<exact supported model ID 3>" },
    { "family": "xai", "id": "<exact supported model ID 4>" }
  ]
}
```

The four model IDs must differ.
Platform values are `linux`, `windows`, `macos-arm64`, and `macos-x64`.
Use each architecture's `muniment-arm64.app.tar.gz` or `muniment-x64.app.tar.gz` on macOS.
Supply the corresponding `.sig` file from the same candidate.

1. Set `MUNIMENT_NATIVE_DISPOSABLE_USER=1` inside the disposable native login.
2. Run the command from the candidate checkout.

```text
node test/e2e/runner/subscriptions.mjs CANDIDATE PACKAGE SIGNATURE INSTALLED_APP LEASES OUTPUT SOURCE_SHA PLATFORM
```

Use absolute paths for the package, signature, installed app, lease file, and output directory.
Keep the lease and output directories outside the checkout.
The app and runtime do not inherit factory secrets, proxies, provider homes, or Pi settings.
The redactor retains its injected-secret checks.
It creates a new app profile for every run and removes that profile after it stops the probe processes.

## Evidence and release scope

A successful check writes `release-acceptance.json`, a platform evidence JSON file, and a native screenshot.
The evidence JSON contains synthetic reply tokens, thread IDs, run IDs, and requested/actual model receipts.
The screenshot shows only the synthetic thread.
The existing screenshot redactor strips metadata before publication.
The runner does not publish raw provider errors, account files, conversation logs, or native stderr.

A missing native runner, package, or lease produces three blocked cases and a nonzero exit code.
A failed reply, changed payload, wrong model, or cleanup failure also returns a nonzero exit code.
Every run replaces stale passing evidence before it checks prerequisites.

Collect each platform's output in the owner release job:

```text
node test/e2e/runner/collect-subscriptions.mjs SOURCE_SHA OUTPUT LINUX_OUTPUT WINDOWS_OUTPUT ARM64_OUTPUT X64_OUTPUT
```

The collector rebuilds the proof from runner receipts and rejects missing or conflicting evidence.
Upload its output as the GitHub artifact `release-acceptance`.
A missing platform produces blocked cases and a nonzero exit code.
The proof covers only `chat`, `direct-model-selection`, and `model-switching`.
It cannot satisfy the full release matrix or mark a release complete.

Run the focused regression checks with:

```text
node --test test/subscription-acceptance.node.mjs
cargo test --manifest-path src-tauri/Cargo.toml -p muniment-core --locked model_router::transport::tests --lib
```

`npm test` includes the JavaScript regression checks. CI also owns the native platform checks.
