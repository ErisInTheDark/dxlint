# es-capitalized-comments

Enforces the initial letter case of comments. By default, comments must not start with a lowercase letter; recognised tool directives and URLs are ignored.

| | |
| --- | --- |
| Upstream | [`es-capitalized-comments`](https://eslint.org/docs/latest/rules/capitalized-comments) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 172 / 178 corpus cases |

## Enable

```dx
import .ts
rule es-capitalized-comments
```

## Examples

Incorrect:

```ts
// lowercase comment
```

Correct:

```ts
// Capitalized comment
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` or `"never"` | `"always"` | Require or forbid initial capitals. |
| `ignorePattern` | `regex` | `""` | Ignore comments whose starting text matches. |

```dx
rule es-capitalized-comments {
  set mode "never"
  set ignorePattern /TODO/
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-capitalized-comments:line` | Line comments. |
| `es-capitalized-comments:block` | Block comments. |

`rule es-capitalized-comments` turns on every variant; `rule es-capitalized-comments:block off` turns one off.

## Differences from upstream

- The upstream string option is named `mode`. Configure line and block policies through variants rather than an upstream `line`/`block` options object.
- `ignoreInlineComments` and `ignoreConsecutiveComments` are not exposed.
- Six corpus cases rely on ESLint inline configuration or suppression directives. dxlint uses its own configuration and suppression contract.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/capitalized-comments) documentation (MIT).
