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

ci: check test-all doc
    @echo "CI passed"
