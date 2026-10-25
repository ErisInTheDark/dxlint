# rs-array-spacing

Requires spaces just inside nonempty single-line Rust array literals. Empty arrays and multiline arrays are left alone; this rule does not format indexing or array types.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-array-spacing
```

## Examples

Incorrect:

```rust
let values = [1, 2];
let zeros = [0; 4];
```

Correct:

```rust
let values = [ 1, 2 ];
let zeros = [ 0; 4 ];
```
