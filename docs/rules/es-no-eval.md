# es-no-eval

Reports calls and references to `eval`. Executing strings as code can introduce security problems and makes the program harder to understand.

| | |
| --- | --- |
| Upstream | [`no-eval`](https://eslint.org/docs/latest/rules/no-eval) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 110 / 110 corpus cases |

## Enable

```dx
import .ts
rule es-no-eval
```

## Examples

Incorrect:

```ts
const value = eval("obj." + key);
const execute = eval;
```

Correct:

```ts
const value = obj[key];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowIndirect` | `boolean` | `false` | Allow indirect eval references and calls; direct calls are still reported. |

```dx
rule es-no-eval {
  set allowIndirect true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-eval` | Identifier references and direct calls. |
| `es-no-eval:member` | Member and indexed access to global eval. |

`rule es-no-eval` turns on every variant; `rule es-no-eval:member off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-eval) documentation (MIT).
