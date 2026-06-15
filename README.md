# beam-init
Init system for beams - [INTERNAL USE]

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
