# es-no-caller

Reports `arguments.caller` and `arguments.callee`. These old APIs are forbidden in strict mode; use a named function for recursion.

| | |
| --- | --- |
| Upstream | [`es-no-caller`](https://eslint.org/docs/latest/rules/no-caller) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious` |
| Parity | 6 / 6 corpus cases |

## Enable

```dx
import .ts
rule es-no-caller
```

## Examples

Incorrect:

```ts
function count(n) {
  if (n > 0) arguments.callee(n - 1);
}
```

Correct:

```ts
function count(n) {
  if (n > 0) count(n - 1);
}
```

## Differences from upstream

- The diagnostic uses `Avoid arguments.caller.` or `Avoid arguments.callee.` rather than upstream's wording.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-caller) documentation (MIT).
