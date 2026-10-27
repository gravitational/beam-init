import os
import pathlib
import signal
import subprocess
import sys
import time

READY = pathlib.Path("/tmp/graceful-worker-ready")
STOPPED = pathlib.Path("/tmp/graceful-worker-stopped")

worker_code = f"""
import pathlib
import signal
import sys
import time

ready = pathlib.Path({str(READY)!r})
stopped = pathlib.Path({str(STOPPED)!r})

def handle_sigterm(signum, frame):
    stopped.write_text("stopped")
    print("WORKER_RECEIVED_SIGTERM", flush=True)
    sys.exit(0)

signal.signal(signal.SIGTERM, handle_sigterm)
ready.write_text("ready")

# Bound the test even if beam-init fails to terminate this service.
deadline = time.monotonic() + 10
while time.monotonic() < deadline:
    time.sleep(0.1)

sys.exit("worker was not terminated")
"""

bootstrap_received_sigterm = False


def handle_bootstrap_sigterm(signum, frame):
    global bootstrap_received_sigterm
    bootstrap_received_sigterm = True
    print("BOOTSTRAP_RECEIVED_SIGTERM", flush=True)


signal.signal(signal.SIGTERM, handle_bootstrap_sigterm)

subprocess.check_call([
    "beamctl",
    "start",
    "--name",
    "graceful-worker",
    "--graceful-termination-period",
    "5",
    "--",
    "python3",
    "-c",
    worker_code,
])

deadline = time.monotonic() + 5
while not READY.exists():
    if time.monotonic() >= deadline:
        sys.exit("worker did not start")
    time.sleep(0.05)

os.kill(1, signal.SIGTERM)

deadline = time.monotonic() + 5
while not (bootstrap_received_sigterm and STOPPED.exists()):
    if time.monotonic() >= deadline:
        sys.exit(
            "shutdown incomplete: "
            f"bootstrap_stopped={bootstrap_received_sigterm}, "
            f"worker_stopped={STOPPED.exists()}"
        )
    time.sleep(0.05)

print("GRACEFUL_SHUTDOWN_COMPLETE", flush=True)
