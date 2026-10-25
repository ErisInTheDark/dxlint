# es-wrap-iife

Requires parentheses around immediately invoked function expressions. The default wraps the whole call, making the immediate invocation visible.

| | |
| --- | --- |
| Upstream | [`@stylistic/wrap-iife`](https://eslint.style/rules/wrap-iife) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 128 / 128 corpus cases |

## Enable

```dx
import .ts
rule es-wrap-iife
```

## Examples

Incorrect:

```js
var value = function () { return 1; }();
var value = (function () { return 1; })();
```

Correct:

```js
var value = (function () { return 1; }());
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"outside"`, `"inside"` or `"any"` | `"outside"` | Wrap the whole call, wrap only the function, or allow either form. |
| `functionPrototypeMethods` | `boolean` | `false` | Also check invocations through `.call()` and `.apply()`. |

```dx
rule es-wrap-iife {
  set mode "inside"
  set functionPrototypeMethods true
}
```

With `"inside"`, use `(function () { return 1; })()`. With `"any"`, either parenthesised form is accepted.

## Differences from upstream

- Set `mode` instead of passing the upstream string option.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/wrap-iife) documentation (MIT).
