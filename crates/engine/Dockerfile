# syntax=docker/dockerfile:1.7
# Moteur mini8n : Rust → WebAssembly. Produit une image « artefact » (FROM scratch)
# contenant /pkg (JS + .wasm), consommée par l'image web.
# Contexte de build : racine du dépôt.

ARG RUST_VERSION=1.98
ARG WASM_BINDGEN_VERSION=0.2.129

FROM rust:${RUST_VERSION}-slim-trixie AS toolchain
ARG WASM_BINDGEN_VERSION
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked \
 && rm -rf /usr/local/cargo/registry

FROM toolchain AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# Tests du moteur puis compilation wasm (le build échoue si un test échoue).
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo test -p mini8n-engine --locked \
 && cargo build -p mini8n-engine --target wasm32-unknown-unknown --release --locked \
 && wasm-bindgen --target web --out-dir /out --out-name mini8n_engine \
      target/wasm32-unknown-unknown/release/mini8n_engine.wasm

FROM scratch AS artifact
COPY --from=build /out /pkg
