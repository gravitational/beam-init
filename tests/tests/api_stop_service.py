import json
import os
import psutil
import subprocess
import time

def process_exists(process_name):
    for proc in psutil.process_iter(['pid', 'name', 'status']):
        info = proc.info
        print(f"{info["pid"]:<2} {info["name"]:<10} {info["status"]}")
        if info["name"] == process_name:
            return True

    return False

subprocess.check_call(["beamctl", "start", "--name", "sleep", "--", "sleep", "10"])

# Wait a bit to ensure the service has started
time.sleep(.1)

assert process_exists("sleep"), "Sleep not started"

subprocess.check_call(["beamctl", "stop", "sleep"])

assert not process_exists("sleep"), "Sleep still up"

# A plain stop does not prune: the service remains in the list of services.
services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "sleep" in services, services

# But explicitly pruning does fully remove the service.
subprocess.check_call(["beamctl", "stop", "sleep", "--prune"])
services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "sleep" not in services, services

# Also stopping with --prune removes the service from the list.
subprocess.check_call(["beamctl", "start", "--name", "prune-me", "--", "sleep", "10"])
time.sleep(.1)
assert process_exists("sleep"), "Sleep not started"

subprocess.check_call(["beamctl", "stop", "prune-me", "--prune"])

assert not process_exists("sleep"), "Sleep still up"

services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "prune-me" not in services, services

# Check that `--prune` also removes a service if it already naturally ended.
subprocess.check_call(["beamctl", "start", "--name", "truth", "--", "true"])
time.sleep(.1)

services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "truth" in services, services

subprocess.check_call(["beamctl", "stop", "truth", "--prune"])

services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "truth" not in services, services
