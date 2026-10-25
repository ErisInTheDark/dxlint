# es-prefer-namespace-keyword

Requires `namespace` rather than the older `module` keyword for TypeScript namespaces. External module declarations using quoted names are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-namespace-keyword`](https://typescript-eslint.io/rules/prefer-namespace-keyword) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 8 / 8 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-namespace-keyword
```

## Examples

Incorrect:

```ts
module Example {}
```

Correct:

```ts
namespace Example {}
declare module 'foo' {}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-namespace-keyword) documentation (MIT).
