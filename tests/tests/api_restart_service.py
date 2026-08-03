import json
import subprocess
import time

name = "sleep"
subprocess.check_call(["beamctl", "start", "--name", name, "--", "sleep", "10"])

time.sleep(.1)

status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["automatic_restart_attempts"] == 0, status
old_pid = status["status"]["Running"]["main_pid"]

# Restart the service.
subprocess.check_call(["beamctl", "restart", name])

time.sleep(.1)

status = json.loads(subprocess.check_output(["beamctl", "--json", "show", name]))
assert status["automatic_restart_attempts"] == 0, status
new_pid = status["status"]["Running"]["main_pid"]

# The service really was restarted, it has a new PID now.
assert old_pid != new_pid
