# Native sign-out

Signing out of Muniment Desktop first makes a best-effort request to revoke the
current native refresh family and its access sessions. The local session is
then cleared while the registered installation identity is retained for a
later sign-in.

Server revocation is bounded and does not make sign-out depend on network
availability. If the service is unavailable or rejects the revocation request,
the desktop still clears its local access token, refresh token, expiries, and
account subject.
