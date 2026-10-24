# rs-parenthesis-spacing

Forbids horizontal whitespace just inside nonempty Rust parentheses. It does not move code across line breaks. Attribute contents and brace-delimited macro bodies are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-parenthesis-spacing
```

## Examples

Incorrect:

```rust
let result = calculate( value );
```

Correct:

```rust
let result = calculate(value);
```
