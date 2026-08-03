import json
import subprocess
import time

import psutil

# Two distinct non-root users. 
UID_A, GID_A = 1001, 1002
UID_B, GID_B = 1003, 1004


def process_exists(process_name):
    for proc in psutil.process_iter(["pid", "name", "status"]):
        info = proc.info
        print(f"{info['pid']:<2} {info['name']:<10} {info['status']}")
        if info["name"] == process_name:
            return True

    return False


def beamctl(args, *, uid, gid):
    """Run beamctl as the given user and return the completed process."""
    return subprocess.run(
        ["beamctl", *args],
        user=uid,
        group=gid,
        capture_output=True,
        text=True,
    )


# Start service as user A.
result = beamctl(["start", "--name", "sleep", "--", "sleep", "30"], uid=UID_A, gid=GID_A)
assert result.returncode == 0, result.stderr

time.sleep(0.1)
assert process_exists("sleep"), "Sleep not started"

# User B can see user A's service in the list.
result = beamctl(["--json", "list"], uid=UID_B, gid=GID_B)
assert result.returncode == 0, result.stderr
assert "sleep" in json.loads(result.stdout), result.stdout

# User B trying to stop it fails.
result = beamctl(["stop", "sleep"], uid=UID_B, gid=GID_B)
assert result.returncode != 0, "user B was allowed to stop user A's service"
assert "Service owned by a different user" in result.stderr, result.stderr

assert process_exists("sleep"), "Sleep was stopped by invalid user"

# User A can stop the service.
result = beamctl(["stop", "sleep"], uid=UID_A, gid=GID_A)
assert result.returncode == 0, result.stderr

assert not process_exists("sleep"), "Sleep still up"
