import re
import subprocess

output = subprocess.check_output(["beamctl", "show", "autostart"])
assert re.fullmatch(rb"autostart \(running PID=\d+\): /etc/beam-init/svc/autostart/run\n", output), output

