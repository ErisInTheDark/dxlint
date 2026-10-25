# es-id-match

Reports identifier names that do not match a regular expression. This lets a project enforce a naming convention without renaming calls to external APIs.

| | |
| --- | --- |
| Upstream | [`es-id-match`](https://eslint.org/docs/latest/rules/id-match) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 100 / 100 corpus cases |

## Enable

```dx
import .ts
rule es-id-match
```

## Examples

With `mode` set to `/^[a-z]+([A-Z][a-z]+)*$/`:

Incorrect:

```ts
const my_favorite_color = "#112C85";
function do_something() {}
```

Correct:

```ts
const myFavoriteColor = "#112C85";
function doSomething() {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `regex` | `"^.+$"` | Required name pattern. The default accepts every nonempty name. |

```dx
rule es-id-match {
  set mode /^[a-z]+([A-Z][a-z]+)*$/
}
```

## Differences from upstream

- The expression is named `mode`, rather than a positional string.
- Upstream's `properties`, `classFields`, `onlyDeclarations` and `ignoreDestructuring` options are not exposed. Property names and class fields are excluded; declarations, references and unaliased destructuring names are checked.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/id-match) documentation (MIT).
