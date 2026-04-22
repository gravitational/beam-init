import httpx
import os
import psutil
import time

transport = httpx.HTTPTransport(uds = "/run/beam-init")
client = httpx.Client(transport = transport)

resp = client.post("http://beam-init/service/sleep", json = { "cmd":"sleep", "args":["10"] })
assert resp.status_code == 200, "%s %s\n%s" %(resp.status_code, resp.headers, resp.text)

# Wait a bit to ensure the service has started
time.sleep(.1)

found_sleep = False
for proc in psutil.process_iter(['pid', 'name', 'status']):
    info = proc.info
    print(f"{info["pid"]:<2} {info["name"]:<10} {info["status"]}")
    if info["name"] == "sleep":
        found_sleep = True
assert found_sleep, "Sleep not started"
