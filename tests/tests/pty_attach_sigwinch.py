import os
import subprocess

for i in range(0, 100):
    # Intentionally don't attach a PTY to delay SIGWINCH
    subprocess.call(["beamctl", "start", "--name", "foo", "--pty", "--", "bash", "-c", "function sigwinch() { echo sigwinch; exit 0; }; trap sigwinch SIGWINCH; touch /tmp/barrier.txt; while true; do true; done"])

    # Wait for SIGWINCH handler to be registered
    while not os.path.isfile("/tmp/barrier.txt"):
        pass

    # Use su --pty to set a controlling tty for beamctl
    output = subprocess.check_output(["su", "--pty", "root", "-c", "beamctl attach foo"])
    print(output)
    assert output.index(b"sigwinch\r\n\r\ndetached from") != -1
    assert output.endswith(b"(exited normally)\r\n")

    # Cleanup for the next test iteration
    subprocess.check_call(["beamctl", "stop", "--prune", "foo"])
    os.remove("/tmp/barrier.txt")
