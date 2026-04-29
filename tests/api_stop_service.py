import os
import psutil
import subprocess
import time

def process_exists(process_name):
    for proc in psutil.process_iter(['pid', 'name', 'status']):
        info = proc.info
        print(f"{info["pid"]:<2} {info["name"]:<10} {info["status"]}")
        if info["name"] == process_name:
            return True

    return False

subprocess.check_call(["beamctl", "start", "sleep", "--", "sleep", "10"])

# Wait a bit to ensure the service has started
time.sleep(.1)

assert process_exists("sleep"), "Sleep not started"

subprocess.check_call(["beamctl", "stop", "sleep"])

assert not process_exists("sleep"), "Sleep still up"
