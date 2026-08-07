import subprocess

for i in range(0, 1000):
    # Use su --pty to set a controlling tty for beamctl
    try:
        output = subprocess.check_output(["su", "--pty", "root", "-c", "beamctl start --pty whoami"])
    except subprocess.SubprocessError as e:
        print(e.output)
        raise
    print(output)
    assert output.endswith(b"(exited normally)\r\n")
