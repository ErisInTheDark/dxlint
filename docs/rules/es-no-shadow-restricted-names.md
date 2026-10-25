# es-no-shadow-restricted-names

Reports bindings that replace restricted names such as `NaN`, `Infinity`, `undefined`, `eval` and `arguments`. Reusing these names can change familiar expressions in surprising ways.

| | |
| --- | --- |
| Upstream | [`es-no-shadow-restricted-names`](https://eslint.org/docs/latest/rules/no-shadow-restricted-names) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 44 / 44 corpus cases |

## Enable

```dx
import .ts
rule es-no-shadow-restricted-names
```

## Examples

Incorrect:

```ts
const undefined = 5;
class Infinity {}
```

Correct:

```ts
const missingValue = 5;
class Unlimited {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `reportGlobalThis` | `boolean` | `true` | Also forbid bindings named `globalThis`. |

```dx
rule es-no-shadow-restricted-names {
  set reportGlobalThis false
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-shadow-restricted-names) documentation (MIT).
