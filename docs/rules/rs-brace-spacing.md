# rs-brace-spacing

Requires spacing before opening braces and inside single-line braces. Empty single-line braces become `{ }`. Brace-delimited macro bodies and attribute contents are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-brace-spacing
```

## Examples

Incorrect:

```rust
fn value (){1}
fn empty () {}
```

Correct:

```rust
fn value () { 1 }
fn empty () { }
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-brace-spacing:before` | Missing space before `{` following a name or closing delimiter. |
| `rs-brace-spacing:inside` | Spaces just inside single-line braces. |

`rule rs-brace-spacing` turns on every variant; `rule rs-brace-spacing:inside off` turns one off.
