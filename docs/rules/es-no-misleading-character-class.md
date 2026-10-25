# es-no-misleading-character-class

Reports character classes that split a visible Unicode character into separate parts. A character class matches individual code points, not a whole combined character.

| | |
| --- | --- |
| Upstream | [`es-no-misleading-character-class`](https://eslint.org/docs/latest/rules/no-misleading-character-class) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 186 / 186 corpus cases |

## Enable

```dx
import .ts
rule es-no-misleading-character-class
```

## Examples

Incorrect:

```ts
/^[A\u0301]$/u;
/^[\uD83D\uDC4D]$/;
```

Correct:

```ts
/^A\u0301$/u;
/^[\uD83D\uDC4D]$/u;
```

## Differences from upstream

- The upstream `allowEscape` option is not exposed.
- dxlint reports the offending sequence, or the computed pattern's location when it cannot map that sequence to written text. Upstream report spans can differ.
- No suggestions are supplied for adding a Unicode flag.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-misleading-character-class) documentation (MIT).
