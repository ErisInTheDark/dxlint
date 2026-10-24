# rs-keyword-spacing

Requires whitespace after Rust keywords before expressions, and before `else`, `as`, `in` or `where` following a closing delimiter. Spacing before braces belongs to `rs-brace-spacing`.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-keyword-spacing
```

## Examples

Incorrect:

```rust
if(condition) { work(); }else { stop(); }
```

Correct:

```rust
if (condition) { work(); } else { stop(); }
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-keyword-spacing:after` | Missing space after a keyword before expression punctuation. |
| `rs-keyword-spacing:before` | Missing space before `else`, `as`, `in` or `where` after a closing delimiter. |

`rule rs-keyword-spacing` turns on every variant; `rule rs-keyword-spacing:before off` turns one off.
