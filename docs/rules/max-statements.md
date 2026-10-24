# max-statements

Reports functions containing too many statements. Statements inside nested functions are counted separately.

| | |
| --- | --- |
| Upstream | [`max-statements`](https://eslint.org/docs/latest/rules/max-statements) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule max-statements
```

## Examples

With `max` set to `2`:

Incorrect:

```ts
function example() {
  const first = 1;
  const second = 2;
  return first + second;
}
```

Correct:

```ts
function example() {
  const first = 1;
  return first + 2;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `10` | Maximum statements per function. |
| `ignoreTopLevelFunctions` | `boolean` | `false` | Skip the top-level function when the file contains exactly one. |

```dx
rule max-statements {
  set max 2
}
```

## Differences from upstream

- Use `max`, not the deprecated upstream alias `maximum`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-statements) documentation (MIT).
