import os
import psutil
import subprocess
import time

subprocess.check_call(["beamctl", "start", "captures_stdout", "--", "echo", "bar"])
output = subprocess.check_output(["beamctl", "logs", "captures_stdout"])
print(output)
assert output == b"bar\n[log stream closed]\n"

subprocess.check_call(["beamctl", "start", "captures_stderr", "--", "sh", "-c", "echo bar >&2"])
output = subprocess.check_output(["beamctl", "logs", "captures_stderr"])
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
