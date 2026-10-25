# es-no-multiple-empty-lines

Limits runs of blank lines to reduce unnecessary scrolling. Separate limits can apply at the beginning and end of a file; blank lines inside template literals are preserved.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-multiple-empty-lines`](https://eslint.style/rules/no-multiple-empty-lines) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 45 / 45 corpus cases |

## Enable

```dx
import .ts
rule es-no-multiple-empty-lines
```

## Examples

Incorrect:

```ts
const foo = 5;



const bar = 3;
```

Correct:

```ts
const foo = 5;

const bar = 3;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `2` | Maximum consecutive blank lines. |
| `maxEOF` | `int(0..) \| null` | `null` | End-of-file limit; `null` uses `max`. |
| `maxBOF` | `int(0..) \| null` | `null` | Beginning-of-file limit; `null` uses `max`. |

```dx
rule es-no-multiple-empty-lines {
  set max 1
  set maxEOF 0
  set maxBOF 0
}
```

A final line ending is not an extra blank line.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-multiple-empty-lines) documentation (MIT).
