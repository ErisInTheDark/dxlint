# newline-per-chained-call

Reports chained calls that need a preceding line break. Short chains can remain on one line.

| | |
| --- | --- |
| Upstream | [`@stylistic/newline-per-chained-call`](https://eslint.style/rules/newline-per-chained-call) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 37 / 37 corpus cases |

## Enable

```dx
import .ts
rule newline-per-chained-call
```

## Examples

Incorrect:

```ts
obj.first().second().third();
```

Correct:

```ts
obj
  .first()
  .second()
  .third();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreChainWithDepth` | `number` | `2` | Allow chains up to this depth. |

```dx
rule newline-per-chained-call {
  set ignoreChainWithDepth 3
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/newline-per-chained-call) documentation (MIT).
