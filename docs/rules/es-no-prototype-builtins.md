# es-no-prototype-builtins

Reports direct calls to `hasOwnProperty`, `isPrototypeOf` and `propertyIsEnumerable` on objects. An object can override these methods or have no prototype at all.

| | |
| --- | --- |
| Upstream | [`es-no-prototype-builtins`](https://eslint.org/docs/latest/rules/no-prototype-builtins) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 47 / 47 corpus cases |

## Enable

```dx
import .ts
rule es-no-prototype-builtins
```

## Examples

Incorrect:

```ts
const hasBar = foo.hasOwnProperty("bar");
const enumerable = foo.propertyIsEnumerable("bar");
```

Correct:

```ts
const hasBar = Object.prototype.hasOwnProperty.call(foo, "bar");
const enumerable = Object.prototype.propertyIsEnumerable.call(foo, "bar");
```

## Differences from upstream

- dxlint reports the property name but does not offer an editor suggestion.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-prototype-builtins) documentation (MIT).
