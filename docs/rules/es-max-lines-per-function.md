# es-max-lines-per-function

Reports functions with too many lines. Unlike a statement count, this catches long expressions spread across many lines.

| | |
| --- | --- |
| Upstream | [`es-max-lines-per-function`](https://eslint.org/docs/latest/rules/max-lines-per-function) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule es-max-lines-per-function
```

## Examples

With `max` set to `3`:

Incorrect:

```ts
function sum() {
  const value = 1;
  return value;
}
```

Correct:

```ts
function sum() {
  return 1;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `50` | Maximum function lines. |
| `skipComments` | `boolean` | `false` | Exclude comment-only lines. |
| `skipBlankLines` | `boolean` | `false` | Exclude blank lines. |

```dx
rule es-max-lines-per-function {
  set max 3
}
```

## Differences from upstream

- Immediately invoked function expressions are skipped. The upstream `IIFEs` option for including them is not exposed.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-lines-per-function) documentation (MIT).
