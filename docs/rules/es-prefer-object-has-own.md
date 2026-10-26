# es-prefer-object-has-own

Reports indirect `hasOwnProperty` calls that can use `Object.hasOwn`. This shorter form needs an ES2022-compatible runtime.

| | |
| --- | --- |
| Upstream | [`prefer-object-has-own`](https://eslint.org/docs/latest/rules/prefer-object-has-own) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 94 / 94 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-object-has-own
```

## Examples

Incorrect:

```ts
Object.prototype.hasOwnProperty.call(obj, 'a');
```

Correct:

```ts
Object.hasOwn(obj, 'a');
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-object-has-own) documentation (MIT).
