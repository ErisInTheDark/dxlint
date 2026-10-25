# es-no-case-declarations

Reports lexical declarations directly inside `case` or `default` clauses. Wrap a clause in braces to keep its bindings local rather than sharing them across the switch.

| | |
| --- | --- |
| Upstream | [`es-no-case-declarations`](https://eslint.org/docs/latest/rules/no-case-declarations) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 19 / 19 corpus cases |

## Enable

```dx
import .ts
rule es-no-case-declarations
```

## Examples

Incorrect:

```ts
switch (value) {
  case 1:
    const result = compute();
    break;
}
```

Correct:

```ts
switch (value) {
  case 1: {
    const result = compute();
    break;
  }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-case-declarations` | Lexical variable declarations. |
| `es-no-case-declarations:declaration` | Function and class declarations. |

`rule es-no-case-declarations` turns on every variant; `rule es-no-case-declarations:declaration off` turns one off.

## Differences from upstream

- dxlint does not offer upstream's suggestion to wrap a clause in braces.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-case-declarations) documentation (MIT).
