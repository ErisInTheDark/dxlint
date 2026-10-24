# yield-star-spacing

Keeps spacing around the star in delegated `yield*` expressions consistent. By default, there is no space before the star and a space after it.

| | |
| --- | --- |
| Upstream | [`@stylistic/yield-star-spacing`](https://eslint.style/rules/yield-star-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 48 / 48 corpus cases |

## Enable

```dx
import .ts
rule yield-star-spacing
```

## Examples

Incorrect:

```js
function* generator() {
  yield *other();
}
```

Correct:

```js
function* generator() {
  yield* other();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"before"`, `"after"`, `"both"` or `"neither"` | `"after"` | Which sides of the star require spacing. |
| `before` | `boolean` or `null` | `null` | Override spacing between `yield` and `*`; `null` uses `mode`. |
| `after` | `boolean` or `null` | `null` | Override spacing between `*` and its argument; `null` uses `mode`. |

```dx
rule yield-star-spacing {
  set mode "both"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `yield-star-spacing:after` | Spacing after `*`. |
| `yield-star-spacing:before` | Spacing before `*`. |

`rule yield-star-spacing` turns on every variant; `rule yield-star-spacing:before off` turns one off.

## Differences from upstream

- Set `mode` instead of passing the upstream string shorthand. The `before` and `after` options can override either side independently.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/yield-star-spacing) documentation (MIT).
