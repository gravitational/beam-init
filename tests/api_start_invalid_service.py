import json
import os
import psutil
import subprocess
import time

output = subprocess.run(["beamctl", "start", "nonexistent"], stderr=subprocess.PIPE).stderr
print(output)
assert output == b"Failed to spawn nonexistent: No such file or directory (os error 2)\n"

# FIXME nul bytes in process arguments are not allowed. test this by directly talking against the api.
# output = subprocess.run(["beamctl", "start", "nul\0"], stderr=subprocess.PIPE).stderr
# print(output)
# assert output == b"Failed to spawn nul\0: data provided contains a nul byte\n"

# A service that failed to spawn sticks around, and `--prune` removes it.
subprocess.run(["beamctl", "start", "--name", "broken", "--", "nonexistent"], stderr=subprocess.PIPE)

services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "broken" in services, services

subprocess.check_call(["beamctl", "stop", "broken", "--prune"])

services = json.loads(subprocess.check_output(["beamctl", "--json", "list"]))
assert "broken" not in services, services
