# no-inner-declarations

Reports function declarations, and optionally `var` declarations, nested below a program or function body. This avoids confusing hoisting behaviour; strict block-scoped functions are allowed by default.

| | |
| --- | --- |
| Upstream | [`no-inner-declarations`](https://eslint.org/docs/latest/rules/no-inner-declarations) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 68 / 68 corpus cases |

## Enable

```dx
import .ts
rule no-inner-declarations
```

## Examples

In a non-strict script:

Incorrect:

```js
if (condition) {
  function update() {}
}
```

Correct:

```js
function update() {}
if (condition) {
  update();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"functions"` or `"both"` | `"functions"` | Check functions alone or functions and `var`. |
| `blockScopedFunctions` | `"allow"` or `"disallow"` | `"allow"` | Allow strict-mode block functions with ECMAScript 2015 or later. |

```dx
rule no-inner-declarations {
  set mode "both"
  set blockScopedFunctions "disallow"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-inner-declarations` | Nested function declarations. |
| `no-inner-declarations:variable` | Nested `var` declarations when mode is `"both"`. |

`rule no-inner-declarations` turns on every variant; `rule no-inner-declarations:variable off` turns one off.

## Differences from upstream

- Upstream's positional `"functions"` or `"both"` setting is named `mode` in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-inner-declarations) documentation (MIT).
