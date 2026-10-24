# no-invalid-this

Reports `this` where it may be undefined in strict code. Methods, constructors, explicit TypeScript `this` parameters, and other recognised bound contexts are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-invalid-this`](https://typescript-eslint.io/rules/no-invalid-this) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 189 / 189 corpus cases |

## Enable

```dx
import .ts
rule no-invalid-this
```

## Examples

Incorrect:

```ts
"use strict";
function update() {
  this.value = 1;
}
```

Correct:

```ts
class Counter {
  value = 0;
  update() {
    this.value = 1;
  }
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `capIsConstructor` | `boolean` | `true` | Treat capitalised function names as constructors. |

```dx
rule no-invalid-this {
  set capIsConstructor false
}
```

## Differences from upstream

- Combines ESLint's core checks and the TypeScript extension's support for explicit `this` parameters.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-invalid-this) documentation (MIT).
