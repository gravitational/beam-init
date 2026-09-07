import subprocess

subprocess.check_call(["useradd", "foo"])

# Use su --pty to set a controlling tty for beamctl and to change user
# FIXME check beamctl output instead of writing to a file once `beamctl start --pty``
# shows output even when the command immediately finished.
try:
    output = subprocess.check_output(["su", "--pty", "foo", "-c", "beamctl start --pty sh -c 'ls -l $(tty) > /tmp/tty_info'"])
except subprocess.SubprocessError as e:
    print(e.output)
    raise
print(output)

with open("/tmp/tty_info", "r") as file:
    assert file.read().find("crw--w---- 1 foo tty") != -1
