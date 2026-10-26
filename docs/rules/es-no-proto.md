# es-no-proto

Reports access to the legacy `__proto__` property. Use `Object.getPrototypeOf` or `Object.setPrototypeOf` to make the operation explicit.

| | |
| --- | --- |
| Upstream | [`no-proto`](https://eslint.org/docs/latest/rules/no-proto) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 9 / 9 corpus cases |

## Enable

```dx
import .ts
rule es-no-proto
```

## Examples

Incorrect:

```ts
const prototype = obj.__proto__;
obj["__proto__"] = other;
```

Correct:

```ts
const prototype = Object.getPrototypeOf(obj);
Object.setPrototypeOf(obj, other);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-proto) documentation (MIT).
