# A launch smoke cannot prove a service serves

We ship a desktop app with a per-user background service. Our installed
check on one platform was an install-and-launch smoke. It installs the
bundle, launches the app, counts its windows, and captures a screenshot.
That check stayed green for four days while the background service served
nothing at all.

The hole opened through an honest repair. A refactor broke the service
binary's compile on that platform. The repair made it compile again by
recording a failed activation and exiting, with the real serving path left
for a follow-up. The follow-up landed four days later. In between, every
nightly installed a bundle whose service started, wrote one record, and
exited. The app fell back to its degraded local behavior, the smoke
launched it, the windows appeared, and the lane passed. A settings change
a user saved in that window also applied nothing, because the schedule
that applies it lives inside the service.

The fix probes the served contract rather than the process. The smoke now
asks the service's endpoint for its handshake and fails the lane when no
endpoint answers. This is the same lesson as testing a web service by
requesting a route instead of checking that the daemon runs. It is easy to
skip when the smoke's stated job is launch alone.

What this demonstrates: a green launch proves launch. Only a request
against the served interface proves service. Our evidence is four days of
green installed-lane runs over a service that exited at start, closed the
same day a served-endpoint probe joined the lane.

What it does not prove: the probe checks the handshake alone. A service
that answers its handshake and fails deeper operations would still pass
the smoke. Coverage for those paths lives in unit and integration tests,
not in the installed lane.
