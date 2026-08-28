import pathlib
import subprocess
import tempfile


with tempfile.TemporaryDirectory() as root:
    root = pathlib.Path(root)
    first = root / "first"
    second = root / "second"
    first.mkdir()
    second.mkdir()

    # execvp should continue searching PATH after finding an unusable candidate.
    blocked = first / "environment-probe"
    blocked.write_text("#!/bin/sh\nexit 99\n")
    blocked.chmod(0o644)

    # Make the probe available only through the PATH supplied to the service.
    (second / "environment-probe").symlink_to("/usr/bin/env")

    configured_path = f"{first}:{second}"

    subprocess.check_call([
        "beamctl",
        "start",
        "--name=environment",
        f"--env=PATH={configured_path}",
        "--env=BEAM_REPEATED=first",
        "--env=BEAM_REPEATED=second",
        "--env=USER_OVERRIDE=true",
        "--",
        "environment-probe",
    ])

    output = subprocess.check_output([
        "beamctl",
        "logs",
        "environment",
    ])
    print(output)

    # user defined env vars via beamctl
    assert f"PATH={configured_path}\n".encode() in output, output
    assert b"BEAM_REPEATED=second\n" in output, output

    # beam-init file defined env vars via files
    assert b"ENV1=true\n" in output, output
    assert b"ENV2=true\n" in output, output

    # beam-init file order override
    assert b"FILE_OVERRIDE=env2\n" in output, output

    # beamctl overrides beam-init file defined var
    assert b"USER_OVERRIDE=true\n" in output, output

    # beamctl repeated definition overridden
    assert b"BEAM_REPEATED=first\n" not in output, output
