# rs-bracket-spacing

Forbids spaces just inside Rust index brackets. Array literal spacing is handled separately by `rs-array-spacing`.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-bracket-spacing
```

## Examples

Incorrect:

```rust
let first = values[ 0 ];
```

Correct:

```rust
let first = values[0];
```
