#!/usr/bin/env python3
import json, socket, subprocess, sys, time

NAME = sys.argv[1] if len(sys.argv) > 1 else "wedge1"
SOCK = "/run/beam-init"

def c(t=6):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(t)
    s.connect(SOCK)
    return s

def get(s, path):
    s.sendall(f"GET {path} HTTP/1.1\r\nHost: b\r\nConnection: close\r\n\r\n".encode())

def beamctl_list(tag):
    try:
        r = subprocess.check_output(["beamctl", "list"], timeout=6)
        out = r.decode(errors="replace").strip().replace("\n", " | ")
        print(f"beamctl list {tag:6} -> {out[:160]}")
        return True
    except subprocess.TimeoutExpired:
        print(f"beamctl list {tag:6} -> NO ANSWER in 6s (event loop frozen)")
        return False

subprocess.check_call([
    "beamctl", "start", "--name", NAME, "--",
    "sh", "-c", f"while :; do echo { "A" * 200 }; done",
])
beamctl_list("BEFORE")

hold = c()
get(hold, "/service/" + NAME + "/logs?follow=true")
time.sleep(3)
trigger = c()
get(trigger, "/service/" + NAME + "/logs")
frozen = not beamctl_list("DURING")

hold.close()
trigger.close()
time.sleep(2)
back = beamctl_list("AFTER")

d = c()
d.sendall(f"DELETE /service/{NAME} HTTP/1.1\r\nHost: b\r\nConnection: close\r\n\r\n".encode())
print("cleanup delete ->", d.recv(200).decode(errors="replace").splitlines()[0])
d.close()

print()
if frozen and back:
    print("CONFIRMED: beamctl dead only while the follow socket is held, alive again after")
    sys.exit(0)
print(f"NOT confirmed (frozen={frozen} recovered={back})")
sys.exit(1)
