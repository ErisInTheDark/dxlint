# no-useless-escape

Reports escapes that do not change a string, template literal, or regular expression's meaning. Necessary escapes are allowed.

| | |
| --- | --- |
| Upstream | [`no-useless-escape`](https://eslint.org/docs/latest/rules/no-useless-escape) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 288 / 288 corpus cases |

## Enable

```dx
import .ts
rule no-useless-escape
```

## Examples

Incorrect:

```ts
const text = "\#";
const pattern = /\!/;
```

Correct:

```ts
const text = "#";
const pattern = /!/;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowRegexCharacters` | `string[]` | `[]` | Allow unnecessary regex escapes for these characters, for example `-` to prevent accidental ranges during later edits. |

```dx
rule no-useless-escape {
  set allowRegexCharacters ["-"]
}
```

## Differences from upstream

- Upstream offers suggestions for removing or preserving an escape. dxlint reports without suggestions.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-escape) documentation (MIT).
