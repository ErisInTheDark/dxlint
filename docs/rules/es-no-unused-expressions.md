# es-no-unused-expressions

Reports expression statements whose values are discarded without a useful effect. Calls, assignments, and other effectful expressions are allowed; TypeScript assertions do not make an otherwise unused expression useful.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unused-expressions`](https://typescript-eslint.io/rules/no-unused-expressions) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 162 / 162 corpus cases |

## Enable

```dx
import .ts
rule es-no-unused-expressions
```

## Examples

Incorrect:

```ts
1 as number;
window!;
```

Correct:

```ts
getValue() as number;
getValue()!;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowShortCircuit` | `boolean` | `false` | Allow short-circuit expressions with an effectful result, such as `ready && run()`. |
| `allowTernary` | `boolean` | `false` | Allow ternaries when both branches are effectful. |
| `allowTaggedTemplates` | `boolean` | `false` | Allow tagged-template expression statements. |
| `enforceForJSX` | `boolean` | `false` | Report unused JSX expressions. |
| `ignoreDirectives` | `boolean` | `false` | Ignore directive prologues, including when targeting ES3. |

```dx
rule es-no-unused-expressions {
  set allowShortCircuit true
  set allowTernary true
}
```

## Differences from upstream

- The upstream TypeScript rule extends ESLint's core rule. dxlint includes both JavaScript and TypeScript expression handling in this rule.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unused-expressions) and [ESLint](https://eslint.org/docs/latest/rules/no-unused-expressions) documentation (MIT).
