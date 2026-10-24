# rs-colon-spacing

Forbids spaces before a colon and requires whitespace after it. Path separators (`::`) are handled separately.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-colon-spacing
```

## Examples

Incorrect:

```rust
let count :u32 = 1;
```

Correct:

```rust
let count: u32 = 1;
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-colon-spacing:before` | Whitespace before a colon. |
| `rs-colon-spacing:after` | Missing whitespace after a colon. |

`rule rs-colon-spacing` turns on every variant; `rule rs-colon-spacing:before off` turns one off.
