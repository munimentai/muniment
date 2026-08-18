# Linux packages

The Debian package installs `muniment-runtime` as a systemd user service. The
package enables the service for `default.target`, so a user manager starts it
at login. Package removal removes that enablement.

The desktop keeps its existing fallback when no user manager runs. It acquires
the shared instance lock and serves the attach endpoint itself.

## Restart policy

The unit carries `Restart=on-failure`, so the user manager restarts the runtime
after a failed exit. `RestartSec=5s` spaces those restarts.
`StartLimitIntervalSec=300s` and `StartLimitBurst=5` bound them. The manager
allows five starts in five minutes. A crash loop that restarts every five
seconds reaches that limit in about twenty seconds. The unit then stays failed,
which is the needs-attention state ADR 0012 asks for.

Clear a failed unit with
`systemctl --user reset-failed muniment-runtime.service`.

The upgrade-refresh exit shares the same window. After an upgrade replaces the
installed payload, the runtime quiesces and exits with status 75.
`Restart=on-failure` then starts the new payload, and that start counts against
the same start limit as a crash. One refresh spends one of the five starts, so a
normal upgrade leaves four for a later fault.
