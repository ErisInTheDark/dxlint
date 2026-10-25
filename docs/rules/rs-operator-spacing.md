# rs-operator-spacing

Adds missing spaces around Rust binary operators, assignment operators and arrows. Bare `<`, `>`, `&` and `|` are left alone to avoid confusing generics, references and closure pipes with binary operators.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-operator-spacing
```

## Examples

Incorrect:

```rust
let total=first+second;
```

Correct:

```rust
let total = first + second;
```
