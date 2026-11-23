import json
import subprocess
import time

import socket, http.client

def req(path, body=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect("/run/beam-init")
    head = f"POST {path} HTTP/1.1\r\nHost: b\r\nConnection: close\r\n"
    if body:
        head += f"Content-Type: application/json\r\nContent-Length: {len(body)}\r\n"
    s.sendall((head + "\r\n" + (body or "")).encode())
    while print(s.recv(4096)):
        pass
    s.close()

req(
    path = "/service/foo",
    body = json.dumps({
        "cmd": "/usr/bin/python3",
        "args": [
            "-c",
            "import os, termios; \
            os.setpgid(0, os.getppid()); \
            os.setsid(); \
            termios.tcflow(1, termios.TCOOFF); \
            [os.write(1, bytes(4096)) for _ in range(64)]"
        ],
        "liveness": None,
        "pty": True,
        "labels": {}
    })
)

time.sleep(0.5)

# Previously this would cause beam-init to hang due to the following sequence of events:
# 1. the service moves to a new session
# 2. the service causes writes to the pty to block by turning off output and then filling the
#    pty buffer
# 3. send a request to beam-init
# 4. beam-init forwards the request to the monitor
# 5. the monitor fails to perform the request as the service moved to a different session
# 6. the monitor tries to write an error to the pty, which blocks
# 7. beam-init blocks until the monitor completes the command and thus blocks on the pty output to
#    be turned on again.
req(path = "/service/foo/notify_pty_attached")
