# consistent-this

Require consistent names for variables that capture `this`. Reserved alias names must also refer to `this`, rather than an unrelated value.

| | |
| --- | --- |
| Upstream | [`consistent-this`](https://eslint.org/docs/latest/rules/consistent-this) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 26 / 26 corpus cases |

## Enable

```dx
import .ts
rule consistent-this
```

## Examples

Incorrect:

```ts
const self = this;
const that = 42;
```

Correct:

```ts
const that = this;
const self = 42;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `names` | `string[]` | `["that"]` | Names reserved for capturing `this`. |

```dx
rule consistent-this {
  set names ["self"]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `consistent-this` | Reserved aliases assigned values other than `this`. |
| `consistent-this:unexpected` | Captures of `this` using other names. |

`rule consistent-this` turns on every variant; `rule consistent-this:unexpected off` turns one off.

## Differences from upstream

- Set upstream's positional alias names through the `names` array.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/consistent-this) documentation (MIT).
