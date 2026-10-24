# rs-generic-spacing

Removes spaces immediately after a generic argument or parameter list's opening `<`. This rule does not check spaces before the closing `>`.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-generic-spacing
```

## Examples

Incorrect:

```rust
let values: Vec< u32> = Vec::new();
```

Correct:

```rust
let values: Vec<u32> = Vec::new();
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-generic-spacing:open` | Whitespace after the opening `<`. |

`rule rs-generic-spacing` turns on every variant; `rule rs-generic-spacing:open off` turns one off.
