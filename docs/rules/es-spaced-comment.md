# es-spaced-comment

Controls whitespace after line and block comment openers. The default requires a space, with optional exceptions for repeated separators and documentation markers.

| | |
| --- | --- |
| Upstream | [`@stylistic/spaced-comment`](https://eslint.style/rules/spaced-comment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 100 / 101 corpus cases |

## Enable

```dx
import .ts
rule es-spaced-comment
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

`style` is the default option: `rule es-spaced-comment set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"always"` | Require or disallow opener spacing. |
| `exceptions` | `string[]` | `[]` | Repeated separator sequences exempt from required spacing. |
| `markers` | `string[]` | `[]` | Prefixes treated as part of the comment opener. |
| `balanced` | boolean | `false` | For the block variant, apply spacing policy before `*/` too. |

```dx
rule es-spaced-comment {
  set style "always"
  set markers ["/"]
}
rule es-spaced-comment:block {
  set balanced true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-spaced-comment:line` | Line comment openers. |
| `es-spaced-comment:block` | Block comment openers and optional closing spacing. |

`rule es-spaced-comment` turns on every variant; `rule es-spaced-comment:line off` turns one off.

## Differences from upstream

- Use `style` for the string policy and variant blocks for separate line/block options, rather than nested upstream `line` and `block` objects.
- ESLint inline configuration comments are not interpreted. This accounts for the unmatched corpus case.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/spaced-comment) documentation (MIT).
