# es-curly

Require a consistent brace policy for conditional and loop bodies. The default requires braces even around a single statement.

| | |
| --- | --- |
| Upstream | [`curly`](https://eslint.org/docs/latest/rules/curly) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 218 / 218 corpus cases |

## Enable

```dx
import .ts
rule es-curly
```

## Examples

Incorrect:

```ts
if (ready) run();
```

Correct:

```ts
if (ready) { run(); }
```

## Options

`mode` is the default option: `rule es-curly set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"all"` / `"multi"` / `"multi-line"` / `"multi-or-nest"` | `"all"` | Require all braces; braces only for multiple statements; braces for multiline bodies; or braces for multiple statements and nested bodies. |
| `style` | `"consistent"` / `null` | `null` | Require matching braces throughout an if/else chain. |

```dx
rule es-curly {
  set mode "multi-line"
  set style "consistent"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-curly:if` | If bodies. |
| `es-curly:else` | Else bodies. |
| `es-curly:loop` | For and while bodies. |
| `es-curly:loop-body` | For-in, for-of and do-while bodies. |

`rule es-curly` turns on every variant; `rule es-curly:else off` turns one off.

## Differences from upstream

- Set the positional brace policy through `mode`, and the optional `consistent` flag through `style`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/curly) documentation (MIT).
