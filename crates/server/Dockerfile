# syntax=docker/dockerfile:1.7
# API mini8n. Image finale distroless : ni shell, ni gestionnaire de paquets, utilisateur non-root.
# Contexte de build : racine du dépôt.

ARG RUST_VERSION=1.98

FROM rust:${RUST_VERSION}-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo test -p mini8n-server --locked \
 && cargo build -p mini8n-server --release --locked \
 && install -D -m 0555 target/release/mini8n-server /out/mini8n-server

FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /out/mini8n-server /usr/local/bin/mini8n-server
USER nonroot:nonroot
ENV BIND_ADDR=0.0.0.0:3000 \
    LOG_FORMAT=json \
    TRUST_PROXY=true
EXPOSE 3000
HEALTHCHECK --interval=15s --timeout=5s --start-period=20s --retries=3 \
  CMD ["/usr/local/bin/mini8n-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/mini8n-server"]
