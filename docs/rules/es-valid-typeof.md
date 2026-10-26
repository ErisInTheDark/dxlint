# es-valid-typeof

Reports comparisons between `typeof` results and invalid type names. Valid names are `"undefined"`, `"object"`, `"boolean"`, `"number"`, `"string"`, `"function"`, `"symbol"` and `"bigint"`.

| | |
| --- | --- |
| Upstream | [`valid-typeof`](https://eslint.org/docs/latest/rules/valid-typeof) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 54 / 54 corpus cases |

## Enable

```dx
import .ts
rule es-valid-typeof
```

## Examples

Incorrect:

```js
typeof value === "strnig";
typeof value === undefined;
```

Correct:

```js
typeof value === "string";
typeof value === "undefined";
typeof value === typeof other;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `requireStringLiterals` | `boolean` | `false` | Allow comparisons only with valid string literals or another `typeof` expression. |

```dx
rule es-valid-typeof {
  set requireStringLiterals true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/valid-typeof) documentation (MIT).
