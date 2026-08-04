import subprocess

for i in range(0, 1000):
    # Use su --pty to set a controlling tty for beamctl
    output = subprocess.check_output(["su", "--pty", "root", "-c", "beamctl start --pty whoami"])
    print(output)
    assert output.endswith(b"(exited normally)\r\n")
