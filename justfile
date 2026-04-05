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
        if [[ "$name" == "rest_api" || "$name" == "axum_server" || "$name" == "json_api" ]]; then
            $RSC build > /dev/null 2>&1 && echo "✓ (build)" || echo "✗ FAILED"
        else
            output=$($RSC run 2>&1 | tail -1)
            echo "✓ ($output)"
        fi
        cd "$ROOT"
    done
    echo "Done."

ci: check test-all doc
    @echo "CI passed"
