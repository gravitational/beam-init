import os
import psutil
import subprocess
import time

output = subprocess.run(["beamctl", "stop", "non_existent"], stderr=subprocess.PIPE).stderr
print(output)
assert output == b"Service non_existent was not found\n"
