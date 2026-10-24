# rs-no-blank-before-close

Removes blank lines immediately before a closing brace or square bracket. Attribute contents and brace-delimited macro bodies are excluded.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-no-blank-before-close
```

## Examples

Incorrect:

```rust
fn main () {
    work();

}
```

Correct:

```rust
fn main () {
    work();
}
```
