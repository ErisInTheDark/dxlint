# es-no-warning-comments

Reports comments beginning with warning terms such as `TODO` or `FIXME`. Use it to catch unfinished work before shipping.

| | |
| --- | --- |
| Upstream | [`no-warning-comments`](https://eslint.org/docs/latest/rules/no-warning-comments) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 59 / 61 corpus cases |

## Enable

```dx
import .ts
rule es-no-warning-comments
```

## Examples

Incorrect:

```ts
// TODO: handle errors
/* FIXME: check this result */
```

Correct:

```ts
// Errors are handled by the caller.
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `terms` | `string[]` | `["todo", "fixme", "xxx"]` | Warning terms, matched as whole words without case sensitivity. |
| `location` | `"start" \| "anywhere"` | `"start"` | Where to look in a comment. |
| `decoration` | Array of one-character strings | `[]` | Characters ignored before a starting warning term. |

```dx
rule es-no-warning-comments {
  set terms ["todo", "fixme"]
  set location "anywhere"
}
```

## Differences from upstream

- Two corpus cases depend on ESLint inline configuration or suppression directives. dxlint uses its own configuration and suppression syntax.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-warning-comments) documentation (MIT).
