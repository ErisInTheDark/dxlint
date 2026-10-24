# max-lines

Reports files with too many lines. Blank lines and comment-only lines can be excluded from the count.

| | |
| --- | --- |
| Upstream | [`max-lines`](https://eslint.org/docs/latest/rules/max-lines) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 46 / 46 corpus cases |

## Enable

```dx
import .ts
rule max-lines
```

## Examples

With `max` set to `3`:

Incorrect:

```ts
let first,
  second,
  third,
  fourth;
```

Correct:

```ts
let first,
  second,
  third;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `300` | Maximum file lines. |
| `skipComments` | `boolean` | `false` | Exclude comment-only lines. |
| `skipBlankLines` | `boolean` | `false` | Exclude blank lines. |

```dx
rule max-lines {
  set max 3
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-lines) documentation (MIT).
