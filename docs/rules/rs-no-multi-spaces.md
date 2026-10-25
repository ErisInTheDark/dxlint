# rs-no-multi-spaces

Replaces runs of two or more horizontal whitespace characters between Rust code tokens with one space. Leading indentation is left alone, as are attribute contents and brace-delimited macro bodies.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-no-multi-spaces
```

## Examples

Incorrect:

```rust
let  value =  1;
```

Correct:

```rust
let value = 1;
```
