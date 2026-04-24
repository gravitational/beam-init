FROM rust:trixie AS builder
COPY . /src
RUN --mount=type=cache,target=/src/target --mount=type=cache,target=/usr/local/cargo/registry cd /src && cargo build && cp target/debug/beam-init /beam-init && cp target/debug/beamctl /beamctl

FROM debian:trixie
RUN apt install --update -y python3 python3-psutil
COPY --from=builder /beam-init /bin/init
COPY --from=builder /beamctl /bin/beamctl

ENTRYPOINT ["/bin/init"]
