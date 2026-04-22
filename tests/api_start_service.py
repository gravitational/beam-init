import os
import psutil
import subprocess
import time

subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])

# Wait a bit to ensure the service has started
time.sleep(.1)

found_sleep = False
for proc in psutil.process_iter(['pid', 'name', 'status']):
    info = proc.info
    print(f"{info["pid"]:<2} {info["name"]:<10} {info["status"]}")
    if info["name"] == "sleep":
        found_sleep = True
assert found_sleep, "Sleep not started"
