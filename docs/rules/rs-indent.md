# rs-indent

Checks block and continuation indentation in Rust. Block contents sit one level deeper than the opening line, with closing delimiters aligned to that line. Multiline strings and block comments are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-indent
```

## Examples

Incorrect:

```rust
fn main () {
let value = 1;
}
```

Correct:

```rust
fn main () {
    let value = 1;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `unit` | `"auto"` or `string` | `"auto"` | One indentation level. Auto uses tabs when surrounding code uses tabs, otherwise four spaces. |

```dx
rule rs-indent {
  set unit "  "
}
```

Continuation lines get another level. Method chains and operator-led lines can retain supported flat or indented layouts, but successive lines in a chain must align. Comments may align with neighbouring code.
