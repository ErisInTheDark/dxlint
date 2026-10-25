# rs-path-spacing

Removes horizontal whitespace around Rust path separators (`::`). Attribute contents and brace-delimited macro bodies are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-path-spacing
```

## Examples

Incorrect:

```rust
let values = Vec :: new();
```

Correct:

```rust
let values = Vec::new();
```
