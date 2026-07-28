import json
import subprocess
import time

def run(*args):
    return subprocess.run(
        ["beamctl", *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )

def services():
    return json.loads(subprocess.check_output(["beamctl", "--json", "list"]))

def main_pid(name):
    status = services()[name]
    assert "Running" in status, status
    return status["Running"]["main_pid"]

subprocess.check_call(["beamctl", "start", "--name", "sleep", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleepy", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "zebra", "--", "sleep", "10"])

time.sleep(.1)

# Unique prefixes work.
for prefix in ["z", "ze", "zeb", "zebra"]:
    output = run("show", prefix).stdout
    assert b"zebra (running PID" in output, output

output = run("show", "b").stdout
assert b"bootstrap (running PID" in output, output

# An ambiguous prefix fails.
result = run("show", "slee")
assert result.returncode == 1, result
assert result.stderr == b"Service slee was not found\n", result.stderr

# Unless it is an exact match.
output = run("show", "sleep").stdout
assert b"sleep (running PID" in output, output

# When there are no prefix matches, that fails.
result = run("show", "nope")
assert result.returncode == 1, result
assert result.stderr == b"Service nope was not found\n", result.stderr

# Tests for various other commands.

result = run("stop", "slee")
assert result.returncode == 1, result
assert result.stderr == b"Service slee was not found\n", result.stderr

status = services()
assert "Running" in status["sleep"], status
assert "Running" in status["sleepy"], status

subprocess.check_call(["beamctl", "freeze", "zeb"])
status = services()
assert "Frozen" in status["zebra"], status

subprocess.check_call(["beamctl", "thaw", "zeb"])
status = services()
assert "Running" in status["zebra"], status

subprocess.check_call(["beamctl", "stop", "zeb"])
status = services()
assert status["zebra"] == "Stopped", status

# A prefix that was previously ambiguous resolves when the other candidates are pruned.
subprocess.check_call(["beamctl", "stop", "sleepy", "--prune"])

status = services()
assert "sleepy" not in status, status

output = run("show", "slee").stdout
assert b"sleep (running PID" in output, output
