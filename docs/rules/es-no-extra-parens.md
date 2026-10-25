# es-no-extra-parens

Reports parentheses that do not affect an expression's meaning. Parentheses needed for precedence or valid syntax are kept.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-extra-parens`](https://eslint.style/rules/no-extra-parens) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 1212 / 1212 corpus cases |

## Enable

```dx
import .ts
rule es-no-extra-parens
```

## Examples

Incorrect:

```ts
a = (b * c);
typeof (a);
```

Correct:

```ts
a = b * c;
typeof a;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `conditionalAssign` | `boolean` | `true` | Check extra parentheses around assignments in conditions. |
| `ternaryOperandBinaryExpressions` | `boolean` | `true` | Check binary operands of ternary expressions. |
| `nestedBinaryExpressions` | `boolean` | `true` | Check nested binary expressions. |
| `returnAssign` | `boolean` | `true` | Check assignments in return statements. |
| `nestedConditionalExpressions` | `boolean` | `true` | Check nested ternary expressions. |
| `enforceForArrowConditionals` | `boolean` | `true` | Check ternary arrow function bodies. |
| `enforceForSequenceExpressions` | `boolean` | `true` | Check sequence expressions. |
| `enforceForNewInMemberExpressions` | `boolean` | `true` | Check `new` expressions used as member receivers. |
| `enforceForFunctionPrototypeMethods` | `boolean` | `true` | Check function receivers of prototype method calls. |
| `allowParensAfterCommentPattern` | `string` | `""` | Allow parentheses following a comment matching this pattern. |

```dx
rule es-no-extra-parens {
  set nestedBinaryExpressions false
  set allowParensAfterCommentPattern "@type"
}
```

## Differences from upstream

- dxlint does not expose the upstream `"functions"` mode, `ignoreJSX`, `allowNodesInSpreadElement` or `ignoredNodes` options.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-extra-parens) documentation (MIT).
