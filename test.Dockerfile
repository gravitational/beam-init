FROM rust:trixie AS builder

ARG TARGETARCH
WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/src/target \
  --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/rustup \
  case "${TARGETARCH}" in \
  amd64) rust_target="x86_64-unknown-linux-musl" ;; \
  arm64) rust_target="aarch64-unknown-linux-musl" ;; \
  *) echo "Unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
  esac \
  && RUSTFLAGS="-Ctarget-feature=+crt-static" \
  cargo build --all-features --locked --target "${rust_target}" \
  && cp "target/${rust_target}/debug/beam-init" /beam-init \
  && cp "target/${rust_target}/debug/beamctl" /beamctl

FROM debian:trixie

RUN apt-get install --update -y python3 python3-psutil

COPY --from=builder /beam-init /bin/init
COPY --from=builder /beamctl /bin/beamctl

ENV BEAM_INIT_ENABLE_API=1
ENV BEAM_INIT_ENABLE_DEBUG_LOGS=1
ENV RUST_BACKTRACE=1
RUN apt install --update -y gdbserver

ENTRYPOINT ["/bin/init"]
