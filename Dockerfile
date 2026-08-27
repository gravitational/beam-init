FROM rust:trixie AS builder

ARG TARGETARCH
ARG BEAM_VERSION
ARG GIT_SHA
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
  && if [ -z "${BEAM_VERSION}" ]; then unset BEAM_VERSION; fi \
  && if [ -z "${GIT_SHA}" ]; then unset GIT_SHA; fi \
  && RUSTFLAGS="-Ctarget-feature=+crt-static" \
  cargo build --release --locked --target "${rust_target}" \
  && cp "target/${rust_target}/release/beam-init" /beam-init \
  && cp "target/${rust_target}/release/beamctl" /beamctl

FROM debian:trixie

COPY --from=builder /beam-init /usr/local/bin/beam-init
COPY --from=builder /beamctl /usr/local/bin/beamctl

ENTRYPOINT [ "/usr/local/bin/beam-init" ]
