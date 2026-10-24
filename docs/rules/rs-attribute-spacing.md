# rs-attribute-spacing

Removes whitespace between an attribute's `#` or `#!` and its opening bracket. Attribute contents are not reformatted by this rule.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-attribute-spacing
```

## Examples

Incorrect:

```rust
# [derive(Debug)]
struct Item;
```

Correct:

```rust
#[derive(Debug)]
struct Item;
```
