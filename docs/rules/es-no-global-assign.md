# es-no-global-assign

Reports assignments to read-only globals. Overwriting built-in names can break unrelated code that relies on them.

| | |
| --- | --- |
| Upstream | [`es-no-global-assign`](https://eslint.org/docs/latest/rules/no-global-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 18 / 18 corpus cases |

## Enable

```dx
import .ts
rule es-no-global-assign
```

## Examples

Incorrect:

```ts
Object = null;
undefined = 1;
```

Correct:

```ts
let value = 1;
value = 2;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `exceptions` | `string[]` | `[]` | Read-only global names that may be modified. |

```dx
rule es-no-global-assign {
  set exceptions ["Object"]
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-global-assign) documentation (MIT).
