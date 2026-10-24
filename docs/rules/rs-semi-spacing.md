# rs-semi-spacing

Disallows horizontal whitespace before Rust semicolons. The fix removes that whitespace.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-semi-spacing
```

## Examples

Incorrect:

```rust
let count = 1 ;
```

Correct:

```rust
let count = 1;
```

Attribute arguments and brace-delimited macro bodies are not checked.
