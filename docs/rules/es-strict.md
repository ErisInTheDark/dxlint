# es-strict

Requires or disallows `"use strict"` directives. Directives are unnecessary in ECMAScript modules, which already run in strict mode.

| | |
| --- | --- |
| Upstream | [`es-strict`](https://eslint.org/docs/latest/rules/strict) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 127 / 128 corpus cases |

## Enable

```dx
import .ts
rule es-strict
```

## Examples

In an ECMAScript module:

Incorrect:

```ts
"use strict";
export const value = 1;
```

Correct:

```ts
export const value = 1;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"safe" \| "never" \| "global" \| "function"` | `"safe"` | Safe chooses global directives for CommonJS and function directives for scripts; never bans directives. Global/function select the required scope. Modules do not need directives. |

```dx
rule es-strict {
  set mode "never"
}
```

Fixes remove unnecessary directives rather than insert missing ones.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.
- ESLint suppression comments are not interpreted; this accounts for the unmatched corpus case.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/strict) documentation (MIT).
