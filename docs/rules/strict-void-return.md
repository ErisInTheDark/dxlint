# strict-void-return

Reports value-returning functions used where a void-returning callback is expected. Ignored values can hide mistakes, especially promises whose rejections are never handled or generators that are never started.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/strict-void-return`](https://typescript-eslint.io/rules/strict-void-return) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 214 / 221 corpus cases |

## Enable

```dx
import .ts
rule strict-void-return
```

## Examples

Incorrect:

```ts
const getNothing: () => void = () => 2137;
```

Correct:

```ts
const getNothing: () => void = () => {};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowReturnAny` | boolean | `false` | Allow callbacks returning `any`. |

```dx
rule strict-void-return {
  set allowReturnAny true
}
```

## Differences from upstream

- Return types of constrained callable type parameters can be missed, such as a callback constrained to `() => number`.
- Contextual callback types on the right of `??=`, `||=` and `&&=` can differ from upstream.
- Very large numeric property names can fail to match their equivalent string keys, affecting callback target lookup.
- Inherited void-return contracts for class methods are not always checked against every implemented or extended declaration.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/strict-void-return) documentation (MIT).
