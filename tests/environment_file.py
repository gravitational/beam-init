import subprocess
import time

# Write an environment file that beam-init is configured to read.
with open("/etc/beam-env", "w") as f:
    f.write("MY_TEST_VAR=hello_from_env\n")
    f.write("# this is a comment\n")
    f.write("QUOTED_VAR=\"quoted value\"\n")
    f.write("\n")
    f.write("ANOTHER=world\n")

# Start a service that echoes the environment variables.
subprocess.check_call(["beamctl", "start", "--name", "env_echo", "--", "sh", "-c", "echo $MY_TEST_VAR; echo $QUOTED_VAR; echo $ANOTHER"])

# Give the service time to run and exit.
time.sleep(0.5)

output = subprocess.check_output(["beamctl", "logs", "env_echo"])
print(output)
assert output == b"hello_from_env\nquoted value\nworld\n[log stream closed]\n", f"unexpected output: {output!r}"

# Verify that editing the file is picked up by a subsequent service spawn.
with open("/etc/beam-env", "w") as f:
    f.write("MY_TEST_VAR=updated_value\n")

subprocess.check_call(["beamctl", "start", "--name", "env_echo2", "--", "sh", "-c", "echo $MY_TEST_VAR"])

time.sleep(0.5)

output = subprocess.check_output(["beamctl", "logs", "env_echo2"])
print(output)
assert output == b"updated_value\n[log stream closed]\n", f"unexpected output: {output!r}"
