# max-statements-per-line

Reports lines containing too many statements. Keeping statements on separate lines makes code easier to scan.

| | |
| --- | --- |
| Upstream | [`@stylistic/max-statements-per-line`](https://eslint.style/rules/max-statements-per-line) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 99 / 99 corpus cases |

## Enable

```dx
import .ts
rule max-statements-per-line
```

## Examples

Incorrect:

```ts
let first; let second;
```

Correct:

```ts
let first;
let second;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(1..)` | `1` | Maximum statements on one line. |

```dx
rule max-statements-per-line {
  set max 2
}
```

## Differences from upstream

- The upstream `ignoredNodes` option is not exposed.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/max-statements-per-line) documentation (MIT).
