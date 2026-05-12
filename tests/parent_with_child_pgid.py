#!/usr/bin/env python3
import os
import time

read_fd, write_fd = os.pipe()

child_pid = os.fork()

if child_pid == 0:
    # We are the child. 
    os.close(read_fd)
    
    # Set the PGID to our PID.
    os.setpgid(0, 0)

    os.write(write_fd, b"ready\n")
    os.close(write_fd)

    time.sleep(5)
    raise SystemExit(0)

# We are the parent.
os.close(write_fd)

# Wait until child has called setpgid(0, 0).
os.read(read_fd, 1024)
os.close(read_fd)

# Move the parent into the child's process group.
os.setpgid(0, child_pid)

# Stick around for a while
time.sleep(10)
