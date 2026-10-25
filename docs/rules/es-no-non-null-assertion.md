# es-no-non-null-assertion

Reports TypeScript's postfix `!` non-null assertion. Prefer checks or optional chaining that account for absent values instead of telling the type system to ignore them.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-non-null-assertion`](https://typescript-eslint.io/rules/no-non-null-assertion) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-no-non-null-assertion
```

## Examples

Incorrect:

```ts
declare const example: { property?: string };
const includesBaz = example.property!.includes("baz");
```

Correct:

```ts
declare const example: { property?: string };
const includesBaz = example.property?.includes("baz") ?? false;
```

## Differences from upstream

- Upstream suggestions for using optional chaining are not supplied.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-non-null-assertion) documentation (MIT).
