import json
import subprocess
import time

subprocess.check_call(["beamctl", "start", "--name", "spam3", "--", "head", "-c", "10000000", "/dev/zero"])

subprocess.check_call(["beamctl", "wait", "--state=exited", "--timeout=5", "spam3"])

output = subprocess.check_output(["beamctl", "show", "spam3", "--json"])
service = json.loads(output)
print(service)
assert "exited" in service["status"], "process has not exited"
