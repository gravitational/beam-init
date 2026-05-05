import os
import psutil
import subprocess
import time
import re

output = subprocess.check_output(["beamctl", "list"])
assert re.fullmatch(rb"bootstrap \(running PID=\d+\)\n", output), output

subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "valid", "--", "true"])
subprocess.check_call(["beamctl", "start", "invalid", "--", "false"])

time.sleep(.1) # Wait a bit to ensure the service has started
output = subprocess.check_output(["beamctl", "list"])

expected = rb"""bootstrap \(running PID=\d+\)
invalid \(failed with exit status: 1\)
sleep \(running PID=\d+\)
valid \(exited normally\)
"""

assert re.fullmatch(expected, output), output
