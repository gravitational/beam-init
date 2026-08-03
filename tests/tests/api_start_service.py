import os
import psutil
import re
import subprocess
import time

our_uid = os.getuid()
our_gid = os.getgid()

subprocess.check_call(["beamctl", "start", "--name", "sleep", "--", "sleep", "10"])

# Wait a bit to ensure the service has started
time.sleep(.1)

found_sleep = False
for proc in psutil.process_iter(['pid', 'name', 'status']):
    info = proc.info
    print(f"{info["pid"]:<2} {info["name"]:<10} {info["status"]}")
    if info["name"] == "sleep":
        found_sleep = True

        uids = proc.uids();
        assert uids.real == our_uid
        assert uids.effective == our_uid
        assert uids.saved == our_uid

        gids = proc.gids();
        assert gids.real == our_gid
        assert gids.effective == our_gid
        assert gids.saved == our_gid

assert found_sleep, "Sleep not started"

# Starting another service with the same name is an error.
output = subprocess.run(["beamctl", "start", "--name", "sleep", "--", "sleep", "10"], stderr=subprocess.PIPE).stderr
print(output)
assert output == b"Service named `sleep` already exists\n"

# Start the same service, but have beamctl auto-generate a name.
output = subprocess.run(["beamctl", "start", "sleep", "10"], stderr=subprocess.PIPE).stderr
print(output)
assert re.fullmatch(rb"Started service [0-9a-f]{16}\n", output), output
