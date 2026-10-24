# no-dynamic-delete

Reports `delete` with a computed key that is not a string or number literal. A `Map` or `Set` is often a clearer choice for a collection whose keys change at runtime.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-dynamic-delete`](https://typescript-eslint.io/rules/no-dynamic-delete) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 21 / 21 corpus cases |

## Enable

```dx
import .ts
rule no-dynamic-delete
```

## Examples

Incorrect:

```ts
const name = "name";
delete container[name];
```

Correct:

```ts
delete container.name;
delete container["name"];
delete container[-1];
```

## Differences from upstream

- dxlint reports the computed key and provides no automatic fix.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-dynamic-delete) documentation (MIT).
