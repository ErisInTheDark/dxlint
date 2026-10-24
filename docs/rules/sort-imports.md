# sort-imports

Checks import declaration order and alphabetical order of named import members. Declarations are grouped by member syntax, then by local name; the rule does not reorder whole declarations.

| | |
| --- | --- |
| Upstream | [`sort-imports`](https://eslint.org/docs/latest/rules/sort-imports) |
| Needs types | no |
| Fix | autofix (members only) |
| Rulesets | none |
| Parity | 61 / 61 corpus cases |

## Enable

```dx
import .ts
rule sort-imports
```

## Examples

Incorrect:

```ts
import { b, a } from "module";
```

Correct:

```ts
import { a, b } from "module";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreCase` | boolean | `false` | Compare local names without letter case. |
| `allowSeparatedGroups` | boolean | `false` | Restart declaration sorting after blank lines. |

```dx
rule sort-imports {
  set ignoreCase true
  set allowSeparatedGroups true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `sort-imports:declarations` | Declaration ordering; no fix. |
| `sort-imports:members` | Named members; fixes supported member lists. |

`rule sort-imports` turns on every variant; `rule sort-imports:declarations off` turns one off.

## Differences from upstream

- Use variant switches instead of `ignoreDeclarationSort` and `ignoreMemberSort`.
- `memberSyntaxSortOrder` is not configurable. The order is side-effect imports, namespace imports, multiple members, then single members.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/sort-imports) documentation (MIT).
