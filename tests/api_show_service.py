import json
import os
import psutil
import re
import subprocess
import time

subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])
time.sleep(.1) # Wait a bit to ensure the service has started
output = subprocess.check_output(["beamctl", "show", "sleep"])
assert re.fullmatch(rb"sleep \(running PID=\d+\): sleep 10\n", output), output

output = subprocess.check_output(["beamctl", "show", "sleep", "--json"])
service = json.loads(output)

assert service["cmd"] == "sleep", service
assert service["args"] == ["10"], service

status = service["status"]
assert "Running" in status, status
assert isinstance(status["Running"]["main_pid"], int), status

subprocess.check_call(["beamctl", "stop", "sleep"])
output = subprocess.check_output(["beamctl", "show", "sleep"])
assert output == b"sleep (stopped): sleep 10\n", output

subprocess.check_call(["beamctl", "start", "invalid", "--", "false"])
time.sleep(.1)
output = subprocess.check_output(["beamctl", "show", "invalid"])
assert output == b"invalid (failed with exit status: 1): false\n", output

output = subprocess.check_output(["beamctl", "show", "sleep", "--json"])
service = json.loads(output)

# Ordering of the --json flag is irrelevant.
output = subprocess.check_output(["beamctl", "--json", "show", "sleep"])
assert json.loads(output) == service

assert service["cmd"] == "sleep", service
assert service["args"] == ["10"], service
assert service["status"] == "Stopped", service
