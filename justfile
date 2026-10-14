run := if path_exists("/run/.containerenv") == "true" { "" } else { "toolbox run -c rust" }
cargo := run + " env RUSTC_WRAPPER= cargo"

setup:
    scripts/fetch-protoc.sh
    scripts/fetch-anki.sh

build *args:
    {{cargo}} build {{args}}

test *args:
    {{cargo}} test {{args}}

check:
    {{cargo}} clippy --workspace --all-targets -- -D warnings
    {{cargo}} fmt --all --check
