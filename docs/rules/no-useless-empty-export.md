# no-useless-empty-export

Reports empty exports in files already made into modules by another import or export. An empty export that is needed to turn a script into a module is allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-useless-empty-export`](https://typescript-eslint.io/rules/no-useless-empty-export) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 21 / 21 corpus cases |

## Enable

```dx
import .ts
rule no-useless-empty-export
```

## Examples

Incorrect:

```ts
export const value = "hello";
export {};
```

Correct:

```ts
export const value = "hello";
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-useless-empty-export) documentation (MIT).
