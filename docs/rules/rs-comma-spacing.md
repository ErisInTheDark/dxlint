# rs-comma-spacing

Forbids spaces before commas and requires whitespace after a comma followed by another item. Commas immediately before closing delimiters need no following space.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-comma-spacing
```

## Examples

Incorrect:

```rust
call(first ,second);
```

Correct:

```rust
call(first, second);
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-comma-spacing:before` | Whitespace before commas. |
| `rs-comma-spacing:after` | Missing whitespace after commas between items. |

`rule rs-comma-spacing` turns on every variant; `rule rs-comma-spacing:before off` turns one off.
