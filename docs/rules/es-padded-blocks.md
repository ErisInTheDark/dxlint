# es-padded-blocks

Controls blank lines just inside blocks, classes and switches. By default, non-empty bodies need a blank line at both ends.

| | |
| --- | --- |
| Upstream | [`@stylistic/padded-blocks`](https://eslint.style/rules/padded-blocks) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 167 / 167 corpus cases |

## Enable

```dx
import .ts
rule es-padded-blocks
```

## Examples

Incorrect:

```ts
if (a) {
  b();
}
```

Correct:

```ts
if (a) {

  b();

}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never" \| "start" \| "end" \| null` | `null` | Global padding policy. |
| `blocks` | Same as `style` | `null` | Statement blocks, functions and static blocks. |
| `switches` | Same as `style` | `null` | Switch bodies. |
| `classes` | Same as `style` | `null` | Class bodies. |
| `allowSingleLineBlocks` | `boolean` | `false` | Allow bodies on a single line. |

`"start"` and `"end"` require padding only at that boundary. When every policy is `null`, the effective policy is `"always"`. When per-kind policies are supplied, unspecified kinds are ignored unless `style` is set.

```dx
rule es-padded-blocks {
  set style "never"
  set allowSingleLineBlocks true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-padded-blocks:top` | Padding after the opening brace. |
| `es-padded-blocks:bottom` | Padding before the closing brace. |

`rule es-padded-blocks` turns on every variant; `rule es-padded-blocks:bottom off` turns one off.

## Differences from upstream

- Use named `style`, `blocks`, `switches` and `classes` options instead of the upstream positional string or object.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/padded-blocks) documentation (MIT).
