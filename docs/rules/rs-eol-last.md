# rs-eol-last

Requires nonempty Rust files to end with one line break and removes extra blank lines at the end. Empty files are left alone.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-eol-last
```

## Examples

Incorrect: a file containing this line with no final line break, or with extra blank lines after it.

```rust
const VALUE: u32 = 1;
```

Correct: the same line followed by exactly one line break.

```rust
const VALUE: u32 = 1;
```
