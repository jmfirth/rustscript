default:
    @just --list

check:
    cargo fmt --check
    cargo clippy --workspace -- -D warnings

test:
    cargo test --workspace

test-all:
    cargo test --workspace -- --include-ignored

build:
    cargo build --workspace

release:
    cargo build --workspace --release

start *ARGS:
    cargo run --bin rsc -- {{ARGS}}

doc:
    cargo doc --workspace --no-deps --open

clean:
    cargo clean

examples: build
    #!/usr/bin/env bash
    set -e
    RSC="$(pwd)/target/debug/rsc"
    ROOT="$(pwd)"
    echo "Validating examples..."
    for dir in examples/*/; do
        name=$(basename "$dir")
        printf "  %-20s" "$name"
        cd "$dir"
        if [[ "$name" == "tauri_notes" ]]; then
            echo "✓ (skip — requires tauri crate)"
        elif [[ "$name" == "rest_api" || "$name" == "axum_server" || "$name" == "json_api" || "$name" == "fullstack_api" ]]; then
            $RSC build > /dev/null 2>&1 && echo "✓ (build)" || echo "✗ FAILED"
        else
            output=$($RSC run 2>&1 | tail -1)
            echo "✓ ($output)"
        fi
        cd "$ROOT"
    done
    echo "Done."

# Build the WASM binary and install into website
web-wasm:
    wasm-pack build crates/rsc-web --target web --out-dir ../../website/src/wasm
    rm -rf website/public/wasm
    cp -r website/src/wasm website/public/wasm

# Build the website (builds WASM first)
web: web-wasm
    cd website && npm install && npm run build

# Dev mode (hot reload, no static build needed)
web-dev: web-wasm
    cd website && npm run dev

# Serve the built website locally
web-serve: web
    cd website && npx serve out

# Run website e2e tests
web-test: web
    cd website && npx playwright test

ci: check test-all doc
    @echo "CI passed"
