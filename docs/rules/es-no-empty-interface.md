# es-no-empty-interface

Reports interfaces with no members and at most one supertype. With no supertype, an empty interface accepts any non-nullish value; with one, it adds nothing to that supertype.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-empty-interface`](https://typescript-eslint.io/rules/no-empty-interface) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |

## Enable

```dx
import .ts
rule es-no-empty-interface
```

## Examples

Incorrect:

```ts
interface Foo {}
interface Bar extends Foo {}
```

Correct:

```ts
interface Foo { name: string; }
interface Bar { age: number; }
interface Baz extends Foo, Bar {}
```

## Differences from upstream

- The upstream `allowSingleExtends` option is not exposed. Use `es-no-empty-object-type` with `allowInterfaces` set to `"with-single-extends"` if needed.
- dxlint reports the interface body and provides no replacement fix or suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-empty-interface) documentation (MIT).
