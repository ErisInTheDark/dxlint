# es-array-callback-return

Reports array callbacks that fail to return a value where one is needed, such as callbacks to `map`, `filter` and `reduce`. It can also reject return values from `forEach` callbacks.

| | |
| --- | --- |
| Upstream | [`es-array-callback-return`](https://eslint.org/docs/latest/rules/array-callback-return) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 230 / 230 corpus cases |

## Enable

```dx
import .ts
rule es-array-callback-return
```

## Examples

Incorrect:

```ts
const doubled = values.map(value => { value * 2; });
```

Correct:

```ts
const doubled = values.map(value => value * 2);
values.forEach(value => { console.log(value); });
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowImplicit` | `boolean` | `false` | Allow bare returns in callbacks expecting a result. |
| `checkForEach` | `boolean` | `false` | Report values returned from `forEach` callbacks. |
| `allowVoid` | `boolean` | `false` | Allow explicitly discarded results using `void` when checking `forEach`. |

```dx
rule es-array-callback-return {
  set checkForEach true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-array-callback-return` | Callbacks with a path that falls through without returning. |
| `es-array-callback-return:concise` | Expression-bodied `forEach` callbacks. |
| `es-array-callback-return:return` | Return statements within callbacks. |

`rule es-array-callback-return` turns on every variant; `rule es-array-callback-return:concise off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/array-callback-return) documentation (MIT).
