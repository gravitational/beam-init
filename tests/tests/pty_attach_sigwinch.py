import subprocess

# Use su --pty to set a controlling tty for beamctl
output = subprocess.check_output(["su", "--pty", "root", "-c", "beamctl start --pty bash -c 'function sigwinch() { echo sigwinch; }; trap sigwinch SIGWINCH; sleep 1'"])
print(output)
assert output.index(b"\r\nsigwinch\r\r\n\r\ndetached from") != -1
assert output.endswith(b"(exited normally)\r\n")
