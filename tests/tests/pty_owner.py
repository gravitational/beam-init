import subprocess

subprocess.check_call(["useradd", "foo"])

# Use su --pty to set a controlling tty for beamctl and to change user
try:
    output = subprocess.check_output(["su", "--pty", "foo", "-c", "beamctl start --pty sh -c 'ls -l $(tty)'"])
except subprocess.SubprocessError as e:
    print(e.output)
    raise
print(output)
assert output.find(b"crw--w---- 1 foo tty") != -1
