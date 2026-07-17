# Install muniment on Windows

muniment ships two Windows installer formats. The NSIS executable is intended
for an individual user. The per-machine MSI is intended for managed deployment
with Microsoft Intune, Group Policy, or another software-management system.

## Per-machine MSI

Choose the file whose name ends in `-machine.msi`. It requires administrator
rights, sets the Windows Installer `ALLUSERS=1` property, installs under
`%ProgramFiles%`, and registers the installation for the machine. Its stable
UpgradeCode allows a newer muniment MSI to upgrade the installed copy in place.

Run these commands from an elevated Command Prompt or deployment agent:

```bat
msiexec.exe /i "muniment-machine.msi" /qn /norestart
msiexec.exe /x "muniment-machine.msi" /qn /norestart
```

`/qn` displays no interface. Use `/qb` for a basic progress interface or `/passive`
for an unattended progress interface. `/norestart` prevents Windows Installer
from restarting the computer; deployment tooling should handle a restart if
`msiexec.exe` returns 3010. Add `/L*v "muniment-install.log"` when a verbose log
is needed. Silent uninstall may also use the MSI product code in place of the
MSI path: `msiexec.exe /x {PRODUCT-CODE} /qn /norestart`.

Do not set `ALLUSERS` on the regular MSI to convert its scope. Use the
`-machine.msi` artifact so upgrades retain the fleet installer's stable identity.

## Per-user NSIS installer

The NSIS installer does not require administrator rights and installs for the
current user under `%LocalAppData%`. The `/S` switch is case-sensitive:

```bat
muniment-nsis.exe /S
"%LocalAppData%\muniment\uninstall.exe" /S
```

The commands return when installation or removal finishes. Run them in the
target user's session so `%LocalAppData%` and the per-user registration refer to
that user.

Nightly files include the exact source commit in their names. Select the
`nightly-<sha>-windows-...-machine.msi` file for managed installation and verify
the release asset came from the expected commit before deployment. Nightly
installers are pre-release builds and are not recommended for a production fleet.
Windows release builds are signed through Azure Artifact Signing; macOS artifacts
remain unsigned until Apple credentials are available.

## WinGet convenience channel

WinGet is an individual/developer convenience channel for stable releases. It
installs the signed per-machine MSI and therefore prompts for administrator
approval; managed fleets should continue to deploy the `-machine.msi` directly
through MDM rather than depend on the community catalog.

After stable promotion succeeds, the release workflow hashes that release's
machine MSI, generates `Muniment.Muniment` under the current winget-pkgs manifest
schema, and opens a **draft** pull request against `microsoft/winget-pkgs`. It
never runs for `nightly`. The owner must create the `mikeydiamonds/winget-pkgs`
fork and configure `WINGET_PKGS_TOKEN` with permission to push to that fork and
open pull requests in the upstream repository. Review and manually mark the PR
ready only after confirming the stable release asset is publicly downloadable.
