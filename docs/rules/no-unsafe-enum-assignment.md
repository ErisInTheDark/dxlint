# no-unsafe-enum-assignment

Reports values that do not share the enum type expected by an assignment, argument, return, or other receiving location. Use named enum members instead of relying on their underlying numbers or strings.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-enum-assignment`](https://typescript-eslint.io/rules/no-unsafe-enum-assignment) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 219 / 249 corpus cases |

## Enable

```dx
import .ts
rule no-unsafe-enum-assignment
```

## Examples

Incorrect:

```ts
enum Fruit { Apple }
const fruit: Fruit = 0;
```

Correct:

```ts
enum Fruit { Apple }
const fruit: Fruit = Fruit.Apple;
```

## Variants

The base check covers assignments, assertions, and mutations.

| Variant | Checks |
| --- | --- |
| `no-unsafe-enum-assignment:access` | Computed keys requiring an enum. |
| `no-unsafe-enum-assignment:arguments` | Function, constructor, and tagged-template arguments. |
| `no-unsafe-enum-assignment:element` | Array elements. |
| `no-unsafe-enum-assignment:jsx` | JSX expressions. |
| `no-unsafe-enum-assignment:returns` | Return values and expression-bodied arrows. |
| `no-unsafe-enum-assignment:spread` | Object spreads. |

`rule no-unsafe-enum-assignment` turns on every variant; `rule no-unsafe-enum-assignment:access off` turns one off.

## Differences from upstream

- Some spread arguments do not retain the enum type expected for each expanded argument.
- Computed and mapped-property index keys can lose their expected enum type, for example `foo[0]` when `foo` is indexed by enum members.
- Generic constraints do not always supply the expected enum type, such as a parameter constrained to `{ fruit: Fruit }`.
- An unannotated field overriding a generic base field can miss the base field's enum requirement.
- Array spreads such as `const fruits: Fruit[] = [...numbers]` can miss unsafe element transfers.
- Array values assigned to a union such as `Fruit | Fruit[]` can miss unsafe elements.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-enum-assignment) documentation (MIT).
