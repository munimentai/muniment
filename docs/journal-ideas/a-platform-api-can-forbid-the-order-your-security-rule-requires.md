# A platform API can forbid the order your security rule requires

We build a desktop product whose background service and its user interface talk over a local, per-user endpoint. On Unix the admission rule is short. The listener accepts a connection, asks the kernel for the peer's user identity, and compares it with its own. It does that before it reads one protocol byte. We wrote that rule into an architecture decision record in plain language.

Then we ported the rule to Windows named pipes, and it stopped being implementable. The server-side call that reads a connected client's identity is `ImpersonateNamedPipeClient`. Microsoft documents that the call adopts the security context of the last message the server read from the pipe. On a byte-mode pipe, a server that has read nothing yet gets `ERROR_CANNOT_IMPERSONATE`. So the identity check needs a prior read, and our written rule forbade any prior read. Two correct-sounding rules produced one impossible order.

Both tempting fixes are bad. Switching the pipe to message mode dodges the constraint, but it forks the wire framing for one platform. Reading the whole first frame before the check hands an unverified peer a parsed protocol message.

We narrowed the exception instead, down to the smallest read the API accepts. Our frames open with a four-byte big-endian length prefix. The listener reads exactly those four bytes. It runs the impersonation check next. Only then does it read the frame body or write any byte. A rejected peer gets its handle closed with no protocol response. A prefix above the frame-size limit gets the same treatment.

The argument for why four bytes are safe is the part worth copying. The prefix carries no protocol authority, because nothing downstream branches on it except a bounds check. The endpoint also has a protected access-control list that grants the creating user alone. Another operating-system user cannot open the pipe to send those four bytes at all. The rule we relaxed is the second line of defense, not the first.

We wrote the exception into the decision record as a dated amendment rather than coding quietly around the old sentence. The amendment names the four-byte prefix as the sole read that may precede the peer check. It then repeats the closing rules for a rejected peer and an oversized prefix. A future reader who finds a read before the check now finds the reason beside it.

Here is the honest limit. ADR 0012 specifies this order, but no integrated Windows session test exercises it yet. The decision record does not prove the four-byte read is harmless against a hostile process running as the same user. We claim no such defense, because same-user isolation sits outside this boundary. We measured no performance effect, because there is nothing here worth measuring.

The transferable lesson is about documents, not pipes. A security rule written on one platform can encode that platform's API contract without anyone noticing. When a port collides with the rule, the useful move is to state the collision, shrink the exception to the smallest thing that clears it, and write down why the remainder still holds.
