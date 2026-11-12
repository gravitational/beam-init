import json
import subprocess
import time
import urllib.error
import urllib.request

import psutil

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

PORT = 8080

server = f"""
import http.server, sys

live = True

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"ok")
        sys.exit(0)

    def log_message(self, *args):
        pass

http.server.HTTPServer(("0.0.0.0", {PORT}), Handler).serve_forever()
"""

# Try start a simple listener with beamctl, but with a funky path -- this will fail
result = subprocess.run(["beamctl", "start", "--name", "listen",
                       "--liveness-initial-delay-seconds", "1",
                       "--liveness-period-seconds", "1",
                       "--liveness-path", "@localhost:80/livez",
                       "--liveness-port", str(PORT),
                       "--liveness-max-retries", "3",
                       "--", "python3", "-c", server])
assert result.returncode != 0, "User can create services with funky paths"

time.sleep(0.5)

# Try start a simple listener with beamctl, but with a unambiguous path
subprocess.check_call(["beamctl", "start", "--name", "listen",
                       "--liveness-initial-delay-seconds", "1",
                       "--liveness-period-seconds", "1",
                       "--liveness-path", "/@localhost:80/livez",
                       "--liveness-port", str(PORT),
                       "--liveness-max-retries", "3",
                       "--", "python3", "-c", server])

subprocess.check_call(["beamctl", "wait", "--state=exited", "--timeout=5", "listen"])

# The server should have exited after the first probe
output = subprocess.check_output(["beamctl", "show", "listen", "--json"])
service = json.loads(output)
assert "exited" in service["status"], "liveness probe was diverted elsewhere"
