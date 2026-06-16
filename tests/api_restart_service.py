import json
import subprocess
import time

name = "sleep"
subprocess.check_call(["beamctl", "start", "--name", name, "--", "sleep", "10"])

time.sleep(.1)

status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["automatic_restart_attempts"] == 0, status

# Restart the service.
subprocess.check_call(["beamctl", "restart", name])

time.sleep(.1)

status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["automatic_restart_attempts"] == 0, status
