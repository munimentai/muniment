# A read page is not a subscription

A companion that wants to inspect one finished run does not need a live event stream.
We added a read-only attach operation that returns the same bounded page the live stream already knows how to build, then stops.

The live stream path already produced a bounded redacted page from sequence 0.
The inspect path calls that same page builder.
It returns the page as the request body.
It creates no subscription id.
It emits no follow-on events.
Contract tests assert the body matches the live page.
Further tests assert no subscription id and no leftover frames on the socket.

That split keeps two different jobs from sharing one lifetime.
A page is a snapshot the caller can drop.
A subscription is a live window the caller must acknowledge, cancel, or let close.
Mixing them would make a one-shot inspect look like a stream that the client then has to shut down.

This does not show that the inspect path is fast.
It does not show editor UX.
It shows only that a bounded read can reuse a stream's page builder without opening a stream.
