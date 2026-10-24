# no-trailing-spaces

Reports whitespace at the end of a line. Removing it keeps source-control diffs free of invisible changes.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-trailing-spaces`](https://eslint.style/rules/no-trailing-spaces) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 60 / 60 corpus cases |

## Enable

```dx
import .ts
rule no-trailing-spaces
```

## Examples

Incorrect (the first line ends with spaces):

```ts
const foo = 0;  
const bar = 5;
```

Correct:

```ts
const foo = 0;
const bar = 5;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `skipBlankLines` | boolean | `false` | Allow whitespace-only lines. |
| `ignoreComments` | boolean | `false` | Allow trailing whitespace on comment lines. |

```dx
rule no-trailing-spaces {
  set skipBlankLines true
  set ignoreComments true
}
```

## Differences from upstream

- In CSS and Markdown corpus examples, dxlint reports the first trailing whitespace character; upstream reports one column earlier. The fixed text is the same.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-trailing-spaces) documentation (MIT).
