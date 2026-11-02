import json
import subprocess
import time

import psutil

UID_A, GID_A = 1001, 1002

# Start service as user
subprocess.check_call(["beamctl", "start", "--name", "victim", "--", "sleep", "1000"], user=UID_A, group=GID_A)

time.sleep(0.1)

output = subprocess.check_output(["beamctl", "show", "victim", "--json"])
service = json.loads(output)
print(service)
victim = psutil.Process(service["status"]["running"]["main_pid"])

# Freeze victim and then kill it
subprocess.check_call(["beamctl", "freeze", "victim"], user=UID_A, group=GID_A)
victim.kill()

# Thaw the dead victim
subprocess.check_call(["beamctl", "thaw", "victim"], user=UID_A, group=GID_A)
