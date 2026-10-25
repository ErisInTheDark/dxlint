# es-default-param-last

Put defaulted and optional parameters after required parameters. Callers can then omit trailing arguments without passing placeholder values.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/default-param-last`](https://typescript-eslint.io/rules/default-param-last) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 183 / 183 corpus cases |

## Enable

```dx
import .ts
rule es-default-param-last
```

## Examples

Incorrect:

```ts
function f(a = 0, b: number) {}
```

Correct:

```ts
function f(a: number, b = 0) {}
function g(a: number, b?: number) {}
```

## Differences from upstream

- Follows the TypeScript extension of the core ESLint rule, including optional parameters and constructor parameter properties.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/default-param-last) documentation (MIT).
