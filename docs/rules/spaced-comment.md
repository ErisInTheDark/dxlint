# spaced-comment

Controls whitespace after line and block comment openers. The default requires a space, with optional exceptions for repeated separators and documentation markers.

| | |
| --- | --- |
| Upstream | [`@stylistic/spaced-comment`](https://eslint.style/rules/spaced-comment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 100 / 101 corpus cases |

## Enable

```dx
import .ts
rule spaced-comment
```

## Examples

Incorrect:

```ts
//This is a comment.
/*This is a comment. */
```

Correct:

```ts
// This is a comment.
/* This is a comment. */
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"always"` | Require or disallow opener spacing. |
| `exceptions` | `string[]` | `[]` | Repeated separator sequences exempt from required spacing. |
| `markers` | `string[]` | `[]` | Prefixes treated as part of the comment opener. |
| `balanced` | boolean | `false` | For the block variant, apply spacing policy before `*/` too. |

```dx
rule spaced-comment {
  set style "always"
  set markers ["/"]
}
rule spaced-comment:block {
  set balanced true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `spaced-comment:line` | Line comment openers. |
| `spaced-comment:block` | Block comment openers and optional closing spacing. |

`rule spaced-comment` turns on every variant; `rule spaced-comment:line off` turns one off.

## Differences from upstream

- Use `style` for the string policy and variant blocks for separate line/block options, rather than nested upstream `line` and `block` objects.
- ESLint inline configuration comments are not interpreted. This accounts for the unmatched corpus case.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/spaced-comment) documentation (MIT).
