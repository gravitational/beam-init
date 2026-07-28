import subprocess
import time
import urllib.error
import urllib.request

PORT = 8081

def get(path):
    try:
        with urllib.request.urlopen(
            f"http://localhost:{PORT}{path}", timeout=1
        ) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as err:
        return err.code, err.read()
    except (urllib.error.URLError, ConnectionError, TimeoutError):
        # The server isn't accepting connections (e.g. it's mid-restart).
        return 0, b""

def logs():
    return subprocess.check_output(["beamctl", "logs", "web"])

server = f"""
import http.server

live = True

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        global live
        if self.path == "/livez":
            self.send_response(200 if live else 500)
            self.end_headers()
            self.wfile.write(b"ok" if live else b"not live")
        elif self.path == "/flip-liveness":
            live = not live
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok" if live else b"not live")
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args):
        pass

http.server.HTTPServer(("0.0.0.0", {PORT}), Handler).serve_forever()
"""

# max retries is 1 here, so we immediately stop the probe on the first failure.
subprocess.check_call(
    [
        "beamctl",
        "start",
        "--name",
        "web",
        "--liveness-path",
        "/livez",
        "--liveness-port",
        str(PORT),
        "--liveness-initial-delay-seconds",
        "1",
        "--liveness-period-seconds",
        "1",
        "--liveness-failure-threshold",
        "3",
        "--liveness-max-retries",
        "1",
        "--",
        "python3",
        "-c",
        server,
    ]
)

# Wait until the server is actually listening and gives a healthy status back.
for _ in range(100):
    status, body = get("/livez")
    if status == 200:
        assert body == b"ok", body
        break
    time.sleep(0.1)
else:
    raise AssertionError("server never became ready")

# All healthy so far. 
assert b"exceeded max retries" not in logs(), logs()

# Now mark the server as unalive.
get("/flip-liveness")
assert get("/livez")[0] == 500

# Once consecutive failures pass max-retries (1) the probe logs the warning.
timeout = time.monotonic() + 5  # seconds
while time.monotonic() < timeout:
    if b"[liveness probe exceeded max retries (max_retries=1)]" in logs():
        break
    time.sleep(0.2)
else:
    raise AssertionError(
        f"probe never logged the max-retries warning: {logs()!r}"
    )

# Having exceeded max-retries the probe stops: waiting another period must not
# produce any further "liveness probe failed" lines.
failures_before = logs().count(b"[liveness probe failed")
time.sleep(1.5)
failures_after = logs().count(b"[liveness probe failed")
assert failures_after == failures_before, (
    f"probe kept failing after exceeding max-retries: {logs()!r}"
)

subprocess.check_call(["beamctl", "stop", "web"])
