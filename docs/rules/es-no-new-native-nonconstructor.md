# es-no-new-native-nonconstructor

Reports `new Symbol` and `new BigInt`. These built-in functions must be called without `new`; attempting to construct them throws.

| | |
| --- | --- |
| Upstream | [`es-no-new-native-nonconstructor`](https://eslint.org/docs/latest/rules/no-new-native-nonconstructor) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 14 / 14 corpus cases |

## Enable

```dx
import .ts
rule es-no-new-native-nonconstructor
```

## Examples

Incorrect:

```ts
const key = new Symbol("key");
const count = new BigInt(42);
```

Correct:

```ts
const key = Symbol("key");
const count = BigInt(42);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-new-native-nonconstructor) documentation (MIT).
