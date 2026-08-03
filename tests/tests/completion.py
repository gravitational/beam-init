import subprocess

def run(*args):
    return subprocess.run(
        ["beamctl", *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )

output = run("completions", "fish").stdout
assert b"Create and start a service" in output, output

# Bash does not store the help text, apparently.
output = run("completions", "bash").stdout
assert b"liveness-initial-delay-seconds" in output, output

output = run("completions", "zsh").stdout
assert b"Create and start a service" in output, output
