#!/usr/bin/env sh
# Compile le moteur Rust en WebAssembly et génère les bindings JS dans web/src/lib/engine/pkg.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/web/src/lib/engine/pkg"
PROFILE="${1:-release}"

# Préférer la toolchain rustup (qui possède la cible wasm32) à un Rust système.
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"

cd "$ROOT"
if [ "$PROFILE" = "release" ]; then
  cargo build -p mini8n-engine --target wasm32-unknown-unknown --release
else
  cargo build -p mini8n-engine --target wasm32-unknown-unknown
fi

wasm-bindgen \
  --target web \
  --out-dir "$OUT" \
  --out-name mini8n_engine \
  "$ROOT/target/wasm32-unknown-unknown/$PROFILE/mini8n_engine.wasm"

# Optimisation facultative si binaryen est installé.
if command -v wasm-opt >/dev/null 2>&1 && [ "$PROFILE" = "release" ]; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    "$OUT/mini8n_engine_bg.wasm" -o "$OUT/mini8n_engine_bg.wasm"
fi

ls -lh "$OUT"/*.wasm
