run := if path_exists("/run/.containerenv") == "true" { "" } else { "toolbox run -c rust" }
cargo := run + " env RUSTC_WRAPPER= cargo"
ui := "crates/cli/ui"

setup:
    scripts/fetch-protoc.sh
    scripts/fetch-anki.sh
    @{{just_executable()}} assets || echo >&2 'warning: the preview UI is not built: install Node.js, then run `just assets`'

assets:
    @command -v npm >/dev/null || { echo >&2 'error: npm not found: install Node.js (see {{ui}}/.nvmrc)'; exit 1; }
    @[ -d {{ui}}/node_modules ] || npm --prefix {{ui}} ci
    npm --prefix {{ui}} run build

build *args:
    @{{just_executable()}} assets || echo >&2 'warning: the preview UI is not built: `janky preview` has no UI, and a release build fails'
    {{cargo}} build {{args}}

test *args:
    {{cargo}} test {{args}}

ui-dev:
    npm --prefix {{ui}} run dev

ui-check:
    npm --prefix {{ui}} run typecheck
    npm --prefix {{ui}} run test

check:
    {{cargo}} clippy --workspace --all-targets -- -D warnings
    {{cargo}} fmt --all --check
    @if command -v npm >/dev/null; then {{just_executable()}} ui-check; else echo >&2 'warning: npm not found: `just ui-check` did not run'; fi
