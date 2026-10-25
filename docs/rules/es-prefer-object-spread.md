# es-prefer-object-spread

Reports `Object.assign` calls whose first argument is an object literal. Object spread makes creating a fresh object clearer; a single object argument needs no `Object.assign` call.

| | |
| --- | --- |
| Upstream | [`es-prefer-object-spread`](https://eslint.org/docs/latest/rules/prefer-object-spread) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 108 / 109 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-object-spread
```

## Examples

Incorrect:

```ts
Object.assign({}, foo);
Object.assign({ foo: 'bar' });
```

Correct:

```ts
({ ...foo });
({ foo: 'bar' });
Object.assign(foo, { bar: baz });
```

## Differences from upstream

- Legacy HTML comments embedded in object literals are not accepted by the TypeScript parser. One upstream script-only corpus case depends on that syntax.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-object-spread) documentation (MIT).
