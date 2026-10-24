# rs-use-list-breaks

Requires one item per line in Rust `use` lists containing paths, globs or nested lists. Simple lists of names may stay on one line.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | none |

## Enable

```dx
import .rs
rule rs-use-list-breaks
```

## Examples

Incorrect:

```rust
use std::{io::Read, fmt};
```

Correct:

```rust
use std::{
    io::Read,
    fmt,
};
use std::io::{Read, Write};
```

Each comma followed by another item on the same line is reported. Commas in nested lists are checked in their own list.
