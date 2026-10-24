# no-whitespace-before-property

Reports spaces around a property-access dot or before a computed-property bracket on the same line. Line breaks in property chains remain allowed.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-whitespace-before-property`](https://eslint.style/rules/no-whitespace-before-property) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 201 / 201 corpus cases |

## Enable

```dx
import .ts
rule no-whitespace-before-property
```

## Examples

Incorrect:

```ts
foo .bar;
foo [bar];
type Item = import("foo") .Bar;
```

Correct:

```ts
foo.bar;
foo[bar];
type Item = import("foo").Bar;
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-whitespace-before-property` | Dot access and qualified type names. |
| `no-whitespace-before-property:computed` | Computed access and indexed-access types. |
| `no-whitespace-before-property:import` | Qualified import types. |

`rule no-whitespace-before-property` turns on every variant; `rule no-whitespace-before-property:computed off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-whitespace-before-property) documentation (MIT).
