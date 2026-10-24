# camelcase

Reports names with underscores between words instead of camel case. Leading and trailing underscores and all-uppercase constant names are allowed.

| | |
| --- | --- |
| Upstream | [`camelcase`](https://eslint.org/docs/latest/rules/camelcase) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 204 / 204 corpus cases |

## Enable

```dx
import .ts
rule camelcase
```

## Examples

Incorrect:

```ts
const my_favourite_colour = 'blue';
```

Correct:

```ts
const myFavouriteColour = 'blue';
const MY_FAVOURITE_COLOUR = 'blue';
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `string[]` | `[]` | Allowed names or regular expression patterns. |
| `ignoreDestructuring` | `boolean` | `false` | Ignore destructured declarations, not all later uses. |
| `ignoreGlobals` | `boolean` | `false` | Ignore global names. |
| `ignoreImports` | `boolean` | `false` | Ignore import declarations, not all later uses. |
| `properties` | `"always"` or `"never"` | `"always"` | Whether to check property names. |

```dx
rule camelcase {
  set properties "never"
  set allow ["legacy_name"]
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/camelcase) documentation (MIT).
