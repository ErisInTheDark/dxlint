# rs-try-spacing

Disallows horizontal whitespace before Rust's `?` operator. Keep error propagation attached to its operand.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-try-spacing
```

## Examples

Incorrect:

```rust
let value = read() ?;
```

Correct:

```rust
let value = read()?;
```

Attribute arguments and brace-delimited macro bodies are not checked.
