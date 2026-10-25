# shared-final-newline

Require exactly one final newline in non-empty files. Unlike `shared-eol-last`, this also removes extra trailing blank lines.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule shared-final-newline
```

## Examples

Incorrect (extra blank lines at the end):

```ts
const answer = 42;


```

Correct (one newline after the semicolon):

```ts
const answer = 42;
```

This language-neutral rule is available through the shared whitespace library. It also reports a non-empty file whose final line has no newline; empty files are left alone.
