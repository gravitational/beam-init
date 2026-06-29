import json
import subprocess
import time
import sys
import urllib.error
import urllib.request

PORT = 8080

def get(path):
    try:
        with urllib.request.urlopen(
            f"http://localhost:{PORT}{path}", timeout=1
        ) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.read()

def show():
    return json.loads(subprocess.check_output(["beamctl", "--json", "show", "web"]))

server = f"""
import http.server

ready = True

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        global ready
        if self.path == "/readyz":
            self.send_response(200 if ready else 500)
            self.end_headers()
            self.wfile.write(b"ok" if ready else b"not ready")
        elif self.path == "/flip-readiness":
            ready = not ready
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok" if ready else b"not ready")
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):
        pass

http.server.HTTPServer(("0.0.0.0", {PORT}), Handler).serve_forever()
"""

# NOTE: the readiness-initial-delay-seconds=1 is kind of load-bearing, otherwise you might
# find that the port is not yet ready to handle connections.
subprocess.check_call(
    [
        "beamctl",
        "start",
        "--name",
        "web",
        "--readiness-path",
        "/readyz",
        "--readiness-port",
        str(PORT),
        "--readiness-initial-delay-seconds",
        "1",
        "--readiness-period-seconds",
        "1",
        "--readiness-failure-threshold",
        "2",
        "--",
        "python3",
        "-c",
        server,
    ]
)


# Wait until the server is actually listening and gives a healthy status back.
for _ in range(100):
    try:
        status, body = get("/readyz")
        if status == 200:
            assert body == b"ok", body
            break
    except (urllib.error.URLError, ConnectionError):
        pass
    time.sleep(0.1)
else:
    raise AssertionError("server never became ready")

# All is healthy, no automatic restarts yet.
service = show()
assert "Running" in service["status"], service
assert service["automatic_restart_attempts"] == 0, service
first_pid = service["status"]["Running"]["main_pid"]

# Now mark the server as unready.
get("/flip-readiness")
assert get("/readyz")[0] == 500

# Wait for the automatic restart.
timeout = time.monotonic() + 5 # seconds
restarted = False
while time.monotonic() < timeout:
    service = show()
    if service["automatic_restart_attempts"] >= 1:
        restarted = True
        break
    time.sleep(0.2)

assert restarted, f"service was not restarted by the readiness probe: {service}"

# After the restart the service runs under a new PID and is healthy again.
for _ in range(100):
    service = show()
    status = service["status"]
    if "Running" in status and status["Running"]["main_pid"] != first_pid:
        break
    time.sleep(0.1)
else:
    raise AssertionError(f"service did not come back under a new PID: {service}")

for _ in range(100):
    if get("/readyz") == (200, b"ok"):
        break
    time.sleep(0.1)
else:
    raise AssertionError("restarted server never became ready again")

subprocess.check_call(["beamctl", "stop", "web"])
