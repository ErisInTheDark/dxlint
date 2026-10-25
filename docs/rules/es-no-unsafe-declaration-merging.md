# es-no-unsafe-declaration-merging

Reports merging a class and interface with the same name. An interface can claim that the class has members which are never created at runtime.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-declaration-merging`](https://typescript-eslint.io/rules/no-unsafe-declaration-merging) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-declaration-merging
```

## Examples

Incorrect:

```ts
interface Foo {}
class Foo {}
```

Correct:

```ts
interface Foo {}
class Bar implements Foo {}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-declaration-merging) documentation (MIT).
