# es-sort-vars

Requires variables within one declaration to be in alphabetical order. Separate declaration statements are not sorted against one another.

| | |
| --- | --- |
| Upstream | [`es-sort-vars`](https://eslint.org/docs/latest/rules/sort-vars) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 61 / 61 corpus cases |

## Enable

```dx
import .ts
rule es-sort-vars
```

## Examples

Incorrect:

```ts
let b, a;
```

Correct:

```ts
let a, b;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreCase` | boolean | `false` | Ignore letter case when comparing variable names. |

```dx
rule es-sort-vars {
  set ignoreCase true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/sort-vars) documentation (MIT).
