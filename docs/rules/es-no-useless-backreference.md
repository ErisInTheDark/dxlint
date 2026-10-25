# es-no-useless-backreference

Reports regular-expression backreferences that can only match the empty string. These often point to the wrong group, a group that has not matched yet, or a group in another alternative.

| | |
| --- | --- |
| Upstream | [`es-no-useless-backreference`](https://eslint.org/docs/latest/rules/no-useless-backreference) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 190 / 190 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-backreference
```

## Examples

Incorrect:

```ts
/\1(a)/;
/(a)|\1b/;
```

Correct:

```ts
/(a)\1/;
/(a)|(b)\2/;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-backreference) documentation (MIT).
