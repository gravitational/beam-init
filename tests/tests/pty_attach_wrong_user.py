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
result = beamctl(["start", "--name", "sleep", "--pty", "--", "sleep", "30"], uid=UID_A, gid=GID_A)
assert result.returncode == 0, result.stderr

time.sleep(0.1)
assert process_exists("sleep"), "Sleep not started"

# Get pty fdstore id
result = beamctl(["--json", "show", "sleep"], uid=UID_A, gid=GID_A)
assert result.returncode == 0, result.stderr
pty_id = json.loads(result.stdout)["status"]["Running"]["pty"][0]
print(pty_id)

# User B trying to get the pty for user A's service fails.
result = beamctl(["testing-get-fd-from-store", str(pty_id)], uid=UID_B, gid=GID_B)
assert result.returncode != 0, "user B was allowed to get pty for user A's service"
assert "fd not found in store" in result.stderr, result.stderr

# User A can get the pty
result = beamctl(["testing-get-fd-from-store", str(pty_id)], uid=UID_A, gid=GID_A)
assert result.returncode == 0, results.stderr
