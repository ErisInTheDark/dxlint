# no-empty-pattern

Reports empty destructuring patterns. They do not bind any variables and can be mistaken for default values.

| | |
| --- | --- |
| Upstream | [`no-empty-pattern`](https://eslint.org/docs/latest/rules/no-empty-pattern) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 31 / 31 corpus cases |

## Enable

```dx
import .ts
rule no-empty-pattern
```

## Examples

Incorrect:

```ts
const { a: {} } = foo;
const [] = items;
```

Correct:

```ts
const { a: { b } } = foo;
const [first] = items;
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-empty-pattern:array` | Empty array patterns. |
| `no-empty-pattern:object` | Empty object patterns. |

`rule no-empty-pattern` turns on every variant; `rule no-empty-pattern:object off` turns one off.

## Differences from upstream

- The upstream `allowObjectPatternsAsParameters` option is not exposed.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-empty-pattern) documentation (MIT).
