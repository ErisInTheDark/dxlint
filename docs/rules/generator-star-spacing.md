# generator-star-spacing

Keep spacing around generator stars consistent. By default a space precedes the star and no space follows it.

| | |
| --- | --- |
| Upstream | [`@stylistic/generator-star-spacing`](https://eslint.style/rules/generator-star-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 207 / 207 corpus cases |

## Enable

```dx
import .ts
rule generator-star-spacing
```

## Examples

Incorrect:

```ts
function* generator() { yield 1; }
```

Correct:

```ts
function *generator() { yield 1; }
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | `boolean` | `true` | Require a space before the star where applicable. |
| `after` | `boolean` | `false` | Require a space after the star. |

```dx
rule generator-star-spacing {
  set before false
  set after true
}
rule generator-star-spacing:anonymous {
  set after false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `generator-star-spacing:named` | Named generator functions. |
| `generator-star-spacing:anonymous` | Anonymous generator functions. |
| `generator-star-spacing:method` | Generator methods. |
| `generator-star-spacing:shorthand` | Object shorthand generator methods. |

`rule generator-star-spacing` turns on every variant; `rule generator-star-spacing:anonymous off` turns one off.

## Differences from upstream

- Use `before` and `after` booleans instead of upstream's string shorthands. Set per-function overrides on the corresponding variants rather than nested option objects.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/generator-star-spacing) documentation (MIT).
