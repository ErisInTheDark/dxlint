# es-no-promise-executor-return

Reports returned values in Promise executor functions. An executor's return value is ignored; call `resolve` or `reject` instead.

| | |
| --- | --- |
| Upstream | [`es-no-promise-executor-return`](https://eslint.org/docs/latest/rules/no-promise-executor-return) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 126 / 126 corpus cases |

## Enable

```dx
import .ts
rule es-no-promise-executor-return
```

## Examples

Incorrect:

```ts
new Promise(() => { return 1; });
new Promise(resolve => resolve(1));
```

Correct:

```ts
new Promise(resolve => { resolve(1); });
Promise.resolve(1);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowVoid` | `boolean` | `false` | Allow explicitly returning a `void` expression. |

```dx
rule es-no-promise-executor-return {
  set allowVoid true
}
```

## Differences from upstream

- dxlint reports the problem without offering editor suggestions.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-promise-executor-return) documentation (MIT).
