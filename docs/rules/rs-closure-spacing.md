# rs-closure-spacing

Keeps closure parameters tight against their pipes and requires whitespace before the body. Attribute contents and brace-delimited macro bodies are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-closure-spacing
```

## Examples

Incorrect:

```rust
let increment = | x |x + 1;
```

Correct:

```rust
let increment = |x| x + 1;
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-closure-spacing` | Whitespace just inside parameter pipes. |
| `rs-closure-spacing:after` | Missing whitespace between parameter pipes and the body. |

`rule rs-closure-spacing` turns on every variant; `rule rs-closure-spacing:after off` turns one off.
