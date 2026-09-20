$ErrorActionPreference = 'Stop'
. $PSScriptRoot/prepare-cef-windows.ps1
cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets
exit $LASTEXITCODE
