# es-yoda

Controls which side of a comparison holds a literal value. By default, literals belong on the right, so comparisons read like `color === "red"` rather than `"red" === color`.

| | |
| --- | --- |
| Upstream | [`yoda`](https://eslint.org/docs/latest/rules/yoda) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 156 / 156 corpus cases |

## Enable

```dx
import .ts
rule es-yoda
```

## Examples

Incorrect:

```js
if ("red" === color) { paint(); }
if (5 > count) { add(); }
```

Correct:

```js
if (color === "red") { paint(); }
if (count < 5) { add(); }
```

## Options

`mode` is the default option: `rule es-yoda set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"never"` or `"always"` | `"never"` | Put the literal on the right or the left. |
| `exceptRange` | `boolean` | `false` | Allow parenthesised ordered range checks such as `(0 <= value && value < 1)`. |
| `onlyEquality` | `boolean` | `false` | Limit checking to `==` and `===`. |

```dx
rule es-yoda {
  set mode "never"
  set exceptRange true
}
```

## Differences from upstream

- Set `mode` instead of passing ESLint's string option.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/yoda) documentation (MIT).
