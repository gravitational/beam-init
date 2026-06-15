import json
import subprocess
import time

name = "sleep"
subprocess.check_call(["beamctl", "start", "--name", name, "--", "sleep", "10"])

time.sleep(.1)

# The service has been started exactly once.
status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["start_attempts"] == 1, status

# Restart the service.
subprocess.check_call(["beamctl", "restart", "sleep"])

time.sleep(.1)

# Restarting starts the service again, so it has now been started twice.
status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["start_attempts"] == 2, status
