# zoomie-lint

Rust linter; rules in `.zlint` (docs/language.md). Binary `zlint`.

## Commands

- test: `cargo test` (unit tests colocated; `tests/cli.rs`, `tests/lsp.rs` drive the binary)
- self-lint: `cargo run --release --` (uses `zoomie.zlint`); must stay clean
- perf check: `zlint --stats <big file>`; compare before/after engine changes

## Layout

- `src/config`: lexer, token stream (imports + macros splice tokens), parser
- `src/pattern`: `Pat`, regex subset parser, compiler, DFA bridge, prefilters
- `src/grammar`: layers per language, `parse.rs` builds trees
- `src/expr`: rule expressions; functions registry in `functions.rs`
- `src/engine`: linter, fixes, suppressions; end-to-end tests in `engine/tests.rs`
- `std/*.zlint`: embedded std library (`src/config/std_lib.rs` lists modules)

## Invariants

- DFA path and interpreter must agree on semantics (looks: `regex-syntax` definitions; see `grammar/text.rs`)
- matchers never match empty; no left recursion
- memoised runs are pure per (matcher, position); `Dead` states rely on it
- new expression functions: add one entry to `FUNCTIONS`
- new std module: add to `std_lib.rs` and docs table
