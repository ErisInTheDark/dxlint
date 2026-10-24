# rs-no-multiple-empty-lines

Limits consecutive blank lines in Rust code to one. A blank line may contain spaces or tabs.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-no-multiple-empty-lines
```

## Examples

Incorrect:

```rust
const FIRST: u32 = 1;


const SECOND: u32 = 2;
```

Correct:

```rust
const FIRST: u32 = 1;

const SECOND: u32 = 2;
```
