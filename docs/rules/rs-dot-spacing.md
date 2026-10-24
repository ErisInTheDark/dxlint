# rs-dot-spacing

Removes horizontal whitespace around dots used for Rust field and method access. It does not join access across line breaks.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-dot-spacing
```

## Examples

Incorrect:

```rust
let name = item . name;
```

Correct:

```rust
let name = item.name;
```
