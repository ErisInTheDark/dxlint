# max-len

Reports lines longer than the configured limit. Separate comment limits and exemptions let you avoid reporting text that cannot usefully be wrapped.

| | |
| --- | --- |
| Upstream | [`@stylistic/max-len`](https://eslint.style/rules/max-len) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 108 / 108 corpus cases |

## Enable

```dx
import .ts
rule max-len
```

## Examples

Incorrect:

```ts
const foo = { "bar": "This is a bar.", "baz": { "qux": "This is a qux" }, "difficult": "to read" };
```

Correct:

```ts
const foo = {
  "bar": "This is a bar.",
  "baz": { "qux": "This is a qux" },
  "difficult": "to read"
};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of numbers or line-limit objects | `[]` | Upstream positional arguments and exemptions. |

The first two numbers set `code` and `tabWidth`. An object accepts these fields:

| Field | Type | Effective default | Meaning |
| --- | --- | --- | --- |
| `code` | `number` | `80` | Maximum line length. |
| `comments` | `number` | `code` | Maximum comment length. |
| `tabWidth` | `number` | `4` | Tab stop width. |
| `ignorePattern` | `string` | No pattern | Skip matching lines. |
| `ignoreComments` | `boolean` | `false` | Ignore comment text. |
| `ignoreTrailingComments` | `boolean` | `false` | Ignore trailing comment text. |
| `ignoreUrls` | `boolean` | `false` | Skip lines containing URLs. |
| `ignoreStrings` | `boolean` | `false` | Skip lines containing quoted strings. |
| `ignoreTemplateLiterals` | `boolean` | `false` | Skip lines containing template literals. |
| `ignoreRegExpLiterals` | `boolean` | `false` | Skip lines containing regular expressions. |

```dx
rule max-len {
  set options [100, { ignoreUrls: true }]
}
```

## Differences from upstream

- Put upstream arguments inside `options`; fields such as `code` are not top-level dxlint options.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/max-len) documentation (MIT).
