import json
import os
import psutil
import re
import subprocess
import time

subprocess.check_call(["beamctl", "start", "--name", "sleep0", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleep1", "--labels", "prio=low", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleep2", "--labels", "prio=urgent,task=dreaming", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleep3", "--labels", "prio=low,task=dreaming", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleep4", "--labels", "task=dreaming", "--", "sleep", "10"])
subprocess.check_call(["beamctl", "start", "--name", "sleep5", "--labels", "prio=urgent,task=dozing_off", "sleep", "10"])
time.sleep(.1) # Wait a bit to ensure the service has started

output = subprocess.check_output(["beamctl", "list", "--selector", "prio=low", "--json"])
services = json.loads(output)
assert set(services) == {"sleep1", "sleep3"}, services

output = subprocess.check_output(["beamctl", "list", "--selector", "prio=urgent", "--json"])
services = json.loads(output)
assert set(services) == {"sleep2", "sleep5"}, services

output = subprocess.check_output(["beamctl", "list", "--selector", "task=dreaming", "--json"])
services = json.loads(output)
assert set(services) == {"sleep2", "sleep3", "sleep4"}, services

output = subprocess.check_output(["beamctl", "list", "--selector", "task=dozing_off", "--json"])
services = json.loads(output)
assert set(services) == {"sleep5"}, services

output = subprocess.check_output(["beamctl", "list", "--selector", "prio=low,task=dreaming", "--json"])
services = json.loads(output)
assert set(services) == {"sleep3"}, services
