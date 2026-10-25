# es-no-confusing-arrow

Reports arrow functions whose conditional expression bodies can look like comparisons. Parentheses or a block body make the meaning clearer.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-confusing-arrow`](https://eslint.style/rules/no-confusing-arrow) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 30 / 30 corpus cases |

## Enable

```dx
import .ts
rule es-no-confusing-arrow
```

## Examples

Incorrect:

```ts
const choose = a => 1 ? 2 : 3;
```

Correct:

```ts
const choose = a => (1 ? 2 : 3);
const other = a => { return 1 ? 2 : 3; };
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowParens` | `boolean` | `true` | Allow a parenthesised conditional body. |
| `onlyOneSimpleParam` | `boolean` | `false` | Check only arrows with one identifier parameter. |

```dx
rule es-no-confusing-arrow {
  set onlyOneSimpleParam true
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-confusing-arrow) documentation (MIT).
