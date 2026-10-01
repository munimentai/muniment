import math
import os
import re
import selectors
import signal
import subprocess
import sys
import time


# Run on the SSH host so a slot timeout also stops the queued driver.
def run(slot_seconds, run_seconds, cleanup_seconds, command):
    child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(child.stdout, selectors.EVENT_READ)
    deadline = time.monotonic() + slot_seconds
    waiting = False
    acquired = False
    pending = b""
    failure = None
    interrupted = False

    def interrupt(signum, frame):
        nonlocal interrupted
        interrupted = True

    signal.signal(signal.SIGTERM, interrupt)
    signal.signal(signal.SIGHUP, interrupt)

    def stop(signum):
        try:
            # sudo forwards SIGTERM to the privileged desktop-ci process.
            os.killpg(child.pid, signum)
        except ProcessLookupError:
            pass

    try:
        while selector.get_map() or child.poll() is None:
            remaining = deadline - time.monotonic()
            if interrupted or remaining <= 0:
                if failure is not None:
                    stop(signal.SIGKILL)
                    break
                failure = "interrupted" if interrupted else (
                    "run" if acquired else "slot-wait" if waiting else "startup")
                interrupted = False
                stop(signal.SIGTERM)
                deadline = time.monotonic() + cleanup_seconds
                continue
            events = selector.select(min(remaining, 0.1))
            for key, _ in events:
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
                # Driver control lines are short. Do not retain artifact payloads.
                lines = (pending + chunk).split(b"\n")
                pending = lines.pop()[-4096:]
                for line in lines:
                    if re.fullmatch(rb"\[desktop-ci \d{2}:\d{2}:\d{2}\] waiting for a desktop-CI slot \(lock\)\.\.\.\r?", line):
                        waiting = True
                    if not acquired and failure is None and re.fullmatch(
                            rb"\[desktop-ci \d{2}:\d{2}:\d{2}\] slot \d+ acquired\r?", line):
                        acquired = True
                        deadline = time.monotonic() + run_seconds
        if failure is not None:
            print(f"\n[subscription-host] timeout={failure}", flush=True)
            return 124
        return child.wait()
    finally:
        selector.close()
        child.stdout.close()
        if child.poll() is None:
            stop(signal.SIGTERM)
            try:
                child.wait(timeout=cleanup_seconds)
            except subprocess.TimeoutExpired:
                stop(signal.SIGKILL)
        child.wait()


if __name__ == "__main__":
    budgets = [float(value) for value in sys.argv[1:4]]
    if len(budgets) != 3 or not all(math.isfinite(value) and value > 0 for value in budgets) or len(sys.argv) < 5:
        raise ValueError("Provide three positive timeout budgets and a command.")
    sys.exit(run(*budgets, sys.argv[4:]))
