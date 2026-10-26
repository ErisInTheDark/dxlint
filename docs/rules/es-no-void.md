# es-no-void

Reports the `void` operator, which evaluates an expression and discards its result. This helps keep intentional uses of `undefined` easy to recognise.

| | |
| --- | --- |
| Upstream | [`no-void`](https://eslint.org/docs/latest/rules/no-void) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 12 / 12 corpus cases |

## Enable

```dx
import .ts
rule es-no-void
```

## Examples

Incorrect:

```ts
const value = void work();
function empty() {
  return void 0;
}
```

Correct:

```ts
work();
function empty() {
  return undefined;
}
```

## Differences from upstream

- The upstream `allowAsStatement` option is not exposed as a dxlint rule option.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-void) documentation (MIT).
