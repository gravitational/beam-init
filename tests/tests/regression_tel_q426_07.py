import json
import subprocess
import time
import socket

UID_A, GID_A = 1001, 1002

# Start the victim
subprocess.check_call(["beamctl", "start", "--name", "victim", "--", "sleep", "100000"])

# Give victim time to start
time.sleep(0.1)

# Create service with malicious name
# Try to create a services with a malicious name
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect("/run/beam-init")
body = json.dumps({"cmd":"/bin/sleep","args":["100000"],"liveness":None,"pty":False,"labels":{}})
s.sendall(f"POST /service/victim%2Fstop%23 HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n{body}".encode())
s.recv(4096)
s.close()

output = subprocess.check_output(["beamctl", "show", "--json", "victim"])
service = json.loads(output)
assert "running" in service["status"]

# This will fail
subprocess.call(["beamctl", "list", "--selector", "foo=bar"])

output = subprocess.check_output(["beamctl", "show", "--json", "victim"])
service = json.loads(output)
assert "running" in service["status"], "Victim was stopped by beamctl list"
