import os
import signal
import subprocess
import time
import psutil
import json

# Check that freeze and thaw work even if the service's parent process changes its PGID.

def find_process_by_pid(pid):
    for proc in psutil.process_iter(["pid", "name", "status"]):
        if proc.info["pid"] == pid:
            return proc

    return None


def assert_process_status(pid, expected_status):
    proc = find_process_by_pid(pid)
    assert proc is not None, f"process {pid} not found"

    proc_status = proc.status()
    assert proc_status == expected_status, (
        f"process {pid} has status {proc_status}, expected {expected_status}"
    )

    return proc

name = "foobar"
subprocess.check_call(["beamctl", "start", name, "--", "python3", "/tests/parent_with_child_pgid.py"])
time.sleep(0.1) # Wait a bit to ensure the service has started.

# The process name is `python3` and not a unique key, so figure out the PID from beamctl.
output = subprocess.check_output(["beamctl", "show", name, "--json"])
parent_pid = json.loads(output)["status"]["Running"]['main_pid']

sleep_proc = assert_process_status(parent_pid, psutil.STATUS_SLEEPING)

subprocess.check_call(["beamctl", "freeze", name])
time.sleep(0.1) # Wait for SIGSTOP to take effect.
sleep_proc = assert_process_status(parent_pid, psutil.STATUS_STOPPED)

subprocess.check_call(["beamctl", "thaw", name])
time.sleep(0.1) # Wait for SIGCONT to take effect.
sleep_proc = assert_process_status(parent_pid, psutil.STATUS_SLEEPING)
