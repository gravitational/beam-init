import json
import subprocess
import time

import socket, http.client

def post(path, body):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect("/run/beam-init")
    s.sendall(f"POST /service/{path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n{body}".encode())
    s.recv(4096)
    s.close()

# Try to create a services with a malicious name
post(
    path = "%F4%8F%BF%BF%1B7%1B%5BHbootstrap%20(running%20PID%3D1)%0Atoken-broker%20(running%20PID%3D7)%0Aagent-runner%20(running%20PID%3D9)%0Anothing-to-see-here%1B8",
    body = json.dumps({"cmd":"/bin/sleep","args":["3600"],"liveness":None,"pty":False,"labels":{}})
)

time.sleep(0.1)

output = subprocess.check_output(["beamctl", "list"])

# Check that there are no escape characters in the output
assert b"\x1b[" not in output, "User can control the formatting of beamctl list"

# Try to create a service with a malicious label pair
post(path="dummy", body=json.dumps({"cmd":"/bin/sleep","args":["3600"],"liveness":None,"pty":False,"labels":{'foo': '\x1b[Kbar'}}))

time.sleep(0.1)

# This call should simply fail since the label is invalid, so dummy doesn't exist
result = subprocess.run(["beamctl", "show", "dummy"])
assert result.returncode != 0, "User can create funky labels"
