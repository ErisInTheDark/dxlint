# es-no-blank-lines-in-optional-blocks

Reports blank lines between an `if` or loop header and its single unbraced statement. The fix keeps the statement on the next line without the blank gap.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule es-no-blank-lines-in-optional-blocks
```

## Examples

Incorrect:

```ts
if (ready)

  run();
```

Correct:

```ts
if (ready)
  run();
```

Applies to `if`, `while`, `for`, `for...in` and `for...of` bodies. Braced bodies and gaps containing comments are not checked.
