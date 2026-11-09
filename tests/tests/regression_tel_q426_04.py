#!/usr/bin/env python3
import json, os, socket, subprocess, sys, time

NAME = sys.argv[1] if len(sys.argv) > 1 else "wedge1"
SOCK = "/run/beam-init"

def http(path, method="GET", t=6):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(t)
    s.connect(SOCK)
    s.sendall(f"{method} {path} HTTP/1.1\r\nHost: b\r\nConnection: close\r\n\r\n".encode())
    return s

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
    "sh", "-c", f"until [ -f /tmp/flood_start_barrier ]; do sleep 0.1; done; for i in $(seq 1 2000); do echo { 'A' * 100 }; done; touch /tmp/flood_end_barrier; sleep 4; echo A",
])
beamctl_list("BEFORE")

hold = http("/service/" + NAME + "/logs?follow=true")
open("/tmp/flood_start_barrier", "w")
while not os.path.isfile("/tmp/flood_end_barrier"): pass
time.sleep(.5)

trigger = http("/service/" + NAME + "/logs")
frozen = not beamctl_list("DURING")

print()
if frozen:
    print("beam-init deadlocked")
    sys.exit(1)

sys.exit(0)
