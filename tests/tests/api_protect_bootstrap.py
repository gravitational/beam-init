import subprocess
import time

# We should not be able to modify bootstrap.

assert subprocess.call(["beamctl", "stop", "bootstrap"]) == 1

# Wait a bit to ensure we have ample time to get killed
time.sleep(.1)

print("Still here")

# Also check some other calls

assert subprocess.call(["beamctl", "freeze", "bootstrap"]) == 1
assert subprocess.call(["beamctl", "restart", "bootstrap"]) == 1
