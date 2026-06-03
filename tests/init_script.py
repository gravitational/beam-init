import json
import re
import subprocess
import time


def show_init_script_json():
    output = subprocess.check_output(["beamctl", "show", "init-script", "--json"])
    return json.loads(output)


service = show_init_script_json()
assert service["cmd"] == "/tests/init_script.sh", service
assert service["args"] == [], service
assert "Running" in service["status"], service

services = json.loads(subprocess.check_output(["beamctl", "list", "--json"]))
assert "Running" in services["init-script"], services

logs = subprocess.check_output(["beamctl", "logs", "init-script"])
assert logs == b"init script started\n", logs

deadline = time.time() + 5
while True:
    output = subprocess.check_output(["beamctl", "show", "init-script"])
    if b"failed with exit status: 42" in output:
        break

    assert time.time() < deadline, output
    time.sleep(0.1)

assert re.fullmatch(
    rb"init-script \(failed with exit status: 42\): /tests/init_script.sh\n",
    output,
), output

deadline = time.time() + 5
while True:
    logs = subprocess.check_output(["beamctl", "logs", "init-script"])
    if logs == b"init script started\ninit script done\n[log stream closed]\n":
        break

    assert time.time() < deadline, logs
    time.sleep(0.1)
