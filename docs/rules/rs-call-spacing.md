# rs-call-spacing

Forbids spaces before call parentheses and index brackets. Function declarations, `Fn` trait syntax and keywords are excluded; declaration spacing has its own rule.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-call-spacing
```

## Examples

Incorrect:

```rust
let result = calculate (value);
let first = values [0];
```

Correct:

```rust
let result = calculate(value);
let first = values[0];
```
