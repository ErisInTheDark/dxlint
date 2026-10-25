# es-space-before-blocks

Requires or disallows spaces before block-opening braces. By default, function, statement, class and module bodies need a preceding space.

| | |
| --- | --- |
| Upstream | [`@stylistic/space-before-blocks`](https://eslint.style/rules/space-before-blocks) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 187 / 187 corpus cases |

## Enable

```dx
import .ts
rule es-space-before-blocks
```

## Examples

Incorrect:

```ts
if (ready){
  run();
}
```

Correct:

```ts
if (ready) {
  run();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"always"` | Default spacing policy. |
| `functions` | `"always" \| "never" \| "off" \| null` | `null` | Function body override; null uses `style`. |
| `keywords` | `"always" \| "never" \| "off" \| null` | `null` | Statement body override. |
| `classes` | `"always" \| "never" \| "off" \| null` | `null` | Class body override. |
| `modules` | `"always" \| "never" \| "off" \| null` | `null` | Module body override. |

```dx
rule es-space-before-blocks {
  set style "always"
  set functions "never"
}
```

## Differences from upstream

- The upstream string policy is named `style`; category overrides are direct options.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/space-before-blocks) documentation (MIT).
