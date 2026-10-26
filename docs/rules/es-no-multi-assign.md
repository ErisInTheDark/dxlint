# es-no-multi-assign

Reports chained assignments. Separate assignments make each changed variable and its scope clearer.

| | |
| --- | --- |
| Upstream | [`no-multi-assign`](https://eslint.org/docs/latest/rules/no-multi-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 31 / 31 corpus cases |

## Enable

```dx
import .ts
rule es-no-multi-assign
```

## Examples

Incorrect:

```ts
let a = b = 5;
```

Correct:

```ts
let a = 5;
let b = 5;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreNonDeclaration` | `boolean` | `false` | Allow chains outside variable and class-field initialisers. |

```dx
rule es-no-multi-assign {
  set ignoreNonDeclaration true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-multi-assign) documentation (MIT).
