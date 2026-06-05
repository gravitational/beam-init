# beam-init
Init system for beams - [INTERNAL USE]

## Testing

> [!NOTE]
> Rust and Docker need to be installed for tests.

Tests are implemented as python scripts in the tests dir. Each test must be listed in `tests/basic.rs`.

To run all tests use:

```shell
cargo test
```

For manual testing you can use:

```shell
docker run -it --rm $(docker build -q -f test.Dockerfile .) bash
```
