import os
import psutil
import subprocess
import time

subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])
time.sleep(.1) # Wait a bit to ensure the service has started
output = subprocess.check_output(["beamctl", "show", "sleep"])
assert output == b"sleep (running PID=10): sleep 10\n", output

subprocess.check_call(["beamctl", "stop", "sleep"])
output = subprocess.check_output(["beamctl", "show", "sleep"])
assert output == b"sleep (stopped): sleep 10\n", output

subprocess.check_call(["beamctl", "start", "invalid", "--", "false"])
time.sleep(.1)
output = subprocess.check_output(["beamctl", "show", "invalid"])
assert output == b"invalid (failed with exit status: 1): false\n", output
