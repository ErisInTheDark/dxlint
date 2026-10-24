# rs-unary-spacing

Disallows horizontal whitespace after Rust unary operators and reference markers. Binary operators and line-leading continuation operators are not its target.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-unary-spacing
```

## Examples

Incorrect:

```rust
let reference = & value;
let negative = - count;
let opposite = ! ready;
```

Correct:

```rust
let reference = &value;
let negative = -count;
let opposite = !ready;
```

Attribute arguments and brace-delimited macro bodies are not checked.
