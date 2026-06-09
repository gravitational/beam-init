import os
import psutil
import subprocess
import time

output = subprocess.run(["beamctl", "start", "foo", "nonexistent"], stderr=subprocess.PIPE).stderr
print(output)
assert output == b"Failed to spawn nonexistent: No such file or directory (os error 2)\n"

# FIXME nul bytes in process arguments are not allowed. test this by directly talking against the api.
# output = subprocess.run(["beamctl", "start", "foo", "nul\0"], stderr=subprocess.PIPE).stderr
# print(output)
# assert output == b"Failed to spawn nul\0: data provided contains a nul byte\n"
