# es-consistent-this

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
rule es-consistent-this
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
rule es-consistent-this {
  set names ["self"]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-consistent-this` | Reserved aliases assigned values other than `this`. |
| `es-consistent-this:unexpected` | Captures of `this` using other names. |

`rule es-consistent-this` turns on every variant; `rule es-consistent-this:unexpected off` turns one off.

## Differences from upstream

- Set upstream's positional alias names through the `names` array.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/consistent-this) documentation (MIT).
