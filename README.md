# beam-init
Init system for [beams](https://beams.run/) and built to run inside containers. `beam-init` is also a service management system that
exposes an HTTP API for managing additional services.

## Usage

Run `beam-init` as the container entrypoint and pass the primary application as
its command. The primary application is registered as the `bootstrap` service,
and the container exits with the same status when that service exits.

```dockerfile
COPY beam-init beamctl /usr/local/bin/

ENV BEAM_INIT_ENABLE_API=1
ENTRYPOINT ["/usr/local/bin/beam-init"]
CMD ["/usr/local/bin/my-app", "--listen", "0.0.0.0:8080"]
```

`BEAM_INIT_ENABLE_API=1` enables the local Unix socket used by `beamctl` to interact with `beam-init`.

```shell
# Start and inspect an additional service.
beamctl start --name my-app -- /usr/local/bin/my-app --listen 0.0.0.0:8080
beamctl list
beamctl show my-app

# Read a log snapshot or follow new output.
beamctl logs my-app
beamctl logs my-app --follow

# Control the service lifecycle.
beamctl freeze my-app
beamctl thaw my-app
beamctl restart my-app
beamctl stop my-app
```

Use `--json` with `list` or `show` for machine-readable output:

```shell
beamctl --json list
beamctl --json show my-app
```

To automatically restart an unhealthy HTTP service, configure a liveness
probe when starting it:

```shell
beamctl start \
  --name my-app \
  --liveness-port 8080 \
  --liveness-path /livez \
  --liveness-initial-delay-seconds 5 \
  --liveness-period-seconds 10 \
  --liveness-failure-threshold 3 \
  -- /usr/local/bin/my-app --listen "0.0.0.0:8080"
```

## Testing

> [!NOTE]
> Rust and Docker need to be installed for tests.

Tests are implemented as Python scripts in the tests dir. Each test must be listed in `tests/basic.rs`.

To run all tests use:

```shell
cargo test
```

For manual testing you can use:

```shell
docker run -it --rm $(docker build -q -f test.Dockerfile .) bash
```

# License

`beam-init` is free software and can be used under the terms of the Apache 2.0 license; see [LICENSE](https://github.com/gravitational/beam-init/blob/master/api/LICENSE) for details.
