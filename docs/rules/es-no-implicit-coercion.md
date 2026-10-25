# es-no-implicit-coercion

Reports shorthand type conversions such as `!!value`, `+value`, and `"" + value`. Explicit conversion functions make the intent clearer.

| | |
| --- | --- |
| Upstream | [`es-no-implicit-coercion`](https://eslint.org/docs/latest/rules/no-implicit-coercion) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 141 / 141 corpus cases |

## Enable

```dx
import .ts
rule es-no-implicit-coercion
```

## Examples

Incorrect:

```ts
const enabled = !!value;
const count = +value;
const text = "" + value;
```

Correct:

```ts
const enabled = Boolean(value);
const count = Number(value);
const text = String(value);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `boolean` | `boolean` | `true` | Check boolean conversions. |
| `number` | `boolean` | `true` | Check numeric conversions. |
| `string` | `boolean` | `true` | Check string conversions. |
| `disallowTemplateShorthand` | `boolean` | `false` | Check templates containing only one expression. |
| `allow` | `string[]` | `[]` | Allow listed shorthand operators, such as `!!` or `+`. |

```dx
rule es-no-implicit-coercion {
  set allow ["!!"]
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-implicit-coercion) documentation (MIT).
