# es-no-inline-comments

Reports comments on the same line as code. Moving comments onto their own lines separates explanation from the statement being read.

| | |
| --- | --- |
| Upstream | [`es-no-inline-comments`](https://eslint.org/docs/latest/rules/no-inline-comments) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule es-no-inline-comments
```

## Examples

Incorrect:

```ts
const count = 1; // Initial count.
```

Correct:

```ts
// Initial count.
const count = 1;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignorePattern` | `regex` | `""` | Ignore matching comment text; empty means no pattern exemption. |

```dx
rule es-no-inline-comments {
  set ignorePattern /TODO/
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-inline-comments) documentation (MIT).
