import os
import signal
import subprocess
import time
import psutil


def find_process(process_name):
    for proc in psutil.process_iter(["pid", "name", "status"]):
        if proc.info["name"] == process_name:
            return proc

    return None


def assert_process_status(process_name, expected_status):
    proc = find_process(process_name)
    assert proc is not None, f"{process_name} not found"

    proc_status = proc.status()
    assert proc_status == expected_status, (
        f"{process_name} has status {proc_status}, expected {expected_status}"
    )

    return proc


subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])
time.sleep(0.1) # Wait a bit to ensure the service has started.
sleep_proc = assert_process_status("sleep", psutil.STATUS_SLEEPING)

subprocess.check_call(["beamctl", "freeze", "sleep"])
time.sleep(0.1) # Wait for SIGSTOP to take effect.
sleep_proc = assert_process_status("sleep", psutil.STATUS_STOPPED)

subprocess.check_call(["beamctl", "thaw", "sleep"])
time.sleep(0.1) # Wait for SIGCONT to take effect.
sleep_proc = assert_process_status("sleep", psutil.STATUS_SLEEPING)

subprocess.check_call(["beamctl", "stop", "sleep"])
time.sleep(0.1)
assert find_process("sleep") is None, "Sleep still up"
