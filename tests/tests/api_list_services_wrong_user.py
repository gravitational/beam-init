import json
import os
import psutil
import re
import subprocess
import time

def beamctl(args, *, uid, gid):
    """Run beamctl as the given user and return the completed process."""
    return subprocess.run(
        ["beamctl", *args],
        user=uid,
        group=gid,
        capture_output=True,
        text=True,
    )

# Two distinct non-root users.
UID_A, GID_A = 1001, 1002
UID_B, GID_B = 1003, 1004

# Start service as user A.
result = beamctl(["start", "--name", "sleep_a", "--", "sleep", "30"], uid=UID_A, gid=GID_A)
assert result.returncode == 0, result.stderr

# Start service as user B.
result = beamctl(["start", "--name", "sleep_b", "--", "sleep", "30"], uid=UID_B, gid=GID_B)

time.sleep(.1) # Wait a bit to ensure the service has started

output = beamctl(["list", "--json"], uid=0, gid=0).stdout
services = json.loads(output)
assert set(services) == {"bootstrap", "sleep_a", "sleep_b"}, services

output = beamctl(["list", "--json"], uid=UID_A, gid=GID_A).stdout
services = json.loads(output)
assert set(services) == {"sleep_a"}, services

output = beamctl(["list", "--json"], uid=UID_B, gid=GID_B).stdout
services = json.loads(output)
assert set(services) == {"sleep_b"}, services
