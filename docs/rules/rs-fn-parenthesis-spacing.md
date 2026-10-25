# rs-fn-parenthesis-spacing

Requires a space between a function declaration's name and its parameter list. It also checks `Fn`, `FnMut` and `FnOnce` parameter syntax. Ordinary calls stay tight under `rs-call-spacing`.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |

## Enable

```dx
import .rs
rule rs-fn-parenthesis-spacing
```

## Examples

Incorrect:

```rust
fn value() -> u32 { 1 }
fn apply<F: Fn(u32) -> u32> (f: F) { f(1); }
```

Correct:

```rust
fn value () -> u32 { 1 }
fn apply<F: Fn (u32) -> u32> (f: F) { f(1); }
```
