# Linux packages

The Debian package installs `muniment-runtime` as a systemd user service. The
package enables the service for `default.target`, so a user manager starts it
at login. Package removal removes that enablement.

The desktop keeps its existing fallback when no user manager runs. It acquires
the shared instance lock and serves the attach endpoint itself.
