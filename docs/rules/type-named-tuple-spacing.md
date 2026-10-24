# type-named-tuple-spacing

Requires named tuple members to use `name: Type` or `name?: Type`. It removes spaces before the colon and requires one space after it.

| | |
| --- | --- |
| Upstream | [`@stylistic/type-named-tuple-spacing`](https://eslint.style/rules/type-named-tuple-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 16 / 16 corpus cases |

## Enable

```dx
import .ts
rule type-named-tuple-spacing
```

## Examples

Incorrect:

```ts
type Point = [x:number, y:number];
type Optional = [value?   :number];
```

Correct:

```ts
type Point = [x: number, y: number];
type Optional = [value?: number];
```

## Variants

| Variant | Checks |
| --- | --- |
| `type-named-tuple-spacing:after` | One space after the colon. |
| `type-named-tuple-spacing:before` | No space after the member name. |
| `type-named-tuple-spacing:optional` | No space between `?` and `:`. |

`rule type-named-tuple-spacing` turns on every variant; `rule type-named-tuple-spacing:optional off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/type-named-tuple-spacing) documentation (MIT).
