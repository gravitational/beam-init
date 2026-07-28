import json
import os
import psutil
import re
import subprocess
import time

output = subprocess.check_output(["beamctl", "list"])
assert re.fullmatch(rb"bootstrap \(running PID=\d+\)\n", output), output

subprocess.check_call(["beamctl", "start", "--name", "sleep", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "valid", "--", "true"])
subprocess.check_call(["beamctl", "start", "--name", "invalid", "--", "false"])

time.sleep(.1) # Wait a bit to ensure the service has started
output = subprocess.check_output(["beamctl", "list"])

expected = rb"""bootstrap \(running PID=\d+\)
invalid \(failed with exit status: 1\)
sleep \(running PID=\d+\)
valid \(exited normally\)
"""

assert re.fullmatch(expected, output), output

output = subprocess.check_output(["beamctl", "list", "--json"])
services = json.loads(output)

# Ordering of the --json flag is irrelevant.
output = subprocess.check_output(["beamctl", "--json", "list"])
assert json.loads(output) == services

assert set(services) == {"bootstrap", "invalid", "sleep", "valid"}, services

bootstrap = services["bootstrap"]
assert "Running" in bootstrap, bootstrap
assert isinstance(bootstrap["Running"]["main_pid"], int), bootstrap

sleep = services["sleep"]
assert "Running" in sleep, sleep
assert isinstance(sleep["Running"]["main_pid"], int), sleep

valid = services["valid"]
assert valid == {"Exited": 0}, valid

invalid = services["invalid"]
assert invalid == {"Exited": 256}, invalid
