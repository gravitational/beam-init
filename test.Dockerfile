FROM rust:trixie AS builder
COPY . /src
RUN --mount=type=cache,target=/src/target cd /src && cargo build && cp target/debug/beam-init /beam-init

FROM debian:trixie
RUN apt install --update -y python3 python3-psutil python3-httpx
COPY --from=builder /beam-init /bin/init

ENTRYPOINT ["/bin/init"]
