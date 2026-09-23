import json
import os
import psutil
import subprocess
import time

output = subprocess.run(["beamctl", "start", "--name", "A" * 256, "true"], stderr=subprocess.PIPE).stderr
print(output)
assert output == b"Service name may not be longer than 255 bytes\n", output

output = subprocess.check_output(["beamctl", "start", "--name", "B" * 255, "true"], stderr=subprocess.PIPE)
print(output)
assert output == b"", output


# Use su --pty to set a controlling tty for beamctl
output = subprocess.check_output(["su", "--pty", "root", "-c" "beamctl start --pty --name " + "C" * 255 + " sleep 1"], stderr=subprocess.PIPE)
print(output)
assert output.startswith(b"Started service CCCC"), output
