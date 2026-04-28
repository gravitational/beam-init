import os
import psutil
import subprocess
import time

subprocess.check_call(["beamctl", "start", "foo", "--", "echo", "bar"])
output = subprocess.check_output(["beamctl", "logs", "foo"])
print(output)
assert output == b"bar\n[log stream closed]\n"

subprocess.check_call(["beamctl", "start", "bar", "--", "sleep", "100"])
output = subprocess.check_output(["beamctl", "logs", "bar"])
print(output)
assert output == b""

subprocess.check_call(["beamctl", "stop", "bar"])
output = subprocess.check_output(["beamctl", "logs", "bar"])
print(output)
assert output == b"[log stream closed]\n"
