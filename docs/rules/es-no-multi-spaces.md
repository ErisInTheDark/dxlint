# es-no-multi-spaces

Reports repeated spaces between code tokens, rather than indentation or spaces inside strings and comments. The fix replaces the gap with one space.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-multi-spaces`](https://eslint.style/rules/no-multi-spaces) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 153 / 153 corpus cases |

## Enable

```dx
import .ts
rule es-no-multi-spaces
```

## Examples

Incorrect:

```ts
const a =  1;
const values = [1,  2];
```

Correct:

```ts
const a = 1;
const values = [1, 2];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `includeTabs` | `boolean` | `true` | Count tabs mixed with spaces as repeated whitespace. |

```dx
rule es-no-multi-spaces {
  set includeTabs false
}
```

## Differences from upstream

- The upstream `ignoreEOLComments` and AST-node `exceptions` options are not exposed by this rule.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-multi-spaces) documentation (MIT).
