import json
import subprocess

name = "exits-after-delay"

subprocess.check_call(
    ["beamctl", "start", "--name", name, "--", "sh", "-c", "sleep 0.2; exit 7"]
)

output = subprocess.check_output(
    [
        "beamctl",
        "wait",
        name,
        "--state",
        "exited",
        "--timeout",
        "5",
        "--json",
    ]
)

status = json.loads(output)
assert status == {"Exited": {"reason": "code", "code": 7}}, status
