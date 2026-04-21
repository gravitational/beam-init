import os
import time

# this creates a zombie child. double fork and wait on the first forked child
# to ensure the second child gets reparented to init
pid = os.fork()
if pid == 0:
    pid = os.fork()
    if pid == 0:
        os._exit(0)
    os._exit(0)

# Reap first child
os.waitpid(pid, 0)
# Wait a bit to ensure the second child turns into a zombie
time.sleep(.1)

import psutil
pid_count = 0
for proc in psutil.process_iter(['pid', 'name', 'status']):
    info = proc.info
    print(f"{info['pid']:<2} {info['name']:<10} {info['status']}")
    pid_count += 1
# We expect two processes. Init and ourself.
assert pid_count == 2, 'Zombie not reaped'
