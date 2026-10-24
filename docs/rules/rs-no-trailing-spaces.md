# rs-no-trailing-spaces

Removes horizontal whitespace at the ends of Rust code lines and on otherwise blank lines. Attribute contents and brace-delimited macro bodies are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-no-trailing-spaces
```

## Examples

Incorrect: spaces or tabs follow the semicolon before the line break.

```rust
let value = 1;
```

Correct: the line ends immediately after the semicolon.

```rust
let value = 1;
```

## Variants

| Variant | Checks |
| --- | --- |
| `rs-no-trailing-spaces` | Whitespace after the last code character on a line. |
| `rs-no-trailing-spaces:blank` | Whitespace on otherwise blank lines. |

`rule rs-no-trailing-spaces` turns on every variant; `rule rs-no-trailing-spaces:blank off` turns one off.
