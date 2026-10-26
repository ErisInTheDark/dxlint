# es-no-throw-literal

Reports thrown expressions that cannot be error objects. Throwing an `Error` preserves useful stack information.

| | |
| --- | --- |
| Upstream | [`no-throw-literal`](https://eslint.org/docs/latest/rules/no-throw-literal) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule es-no-throw-literal
```

## Examples

Incorrect:

```ts
throw "error";
throw null;
```

Correct:

```ts
throw new Error("error");
```

This is a syntax check, not a guarantee that every thrown value is an error. A variable or function call can still produce a non-error value.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-throw-literal) documentation (MIT).
