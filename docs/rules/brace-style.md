# brace-style

Enforces consistent block brace placement. The default uses one true brace style: opening braces follow the statement, and `else`, `catch` and `finally` follow the preceding closing brace.

| | |
| --- | --- |
| Upstream | [`@stylistic/brace-style`](https://eslint.style/rules/brace-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 180 / 180 corpus cases |

## Enable

```dx
import .ts
rule brace-style
```

## Examples

Incorrect:

```ts
if (ready)
{
  run();
}
else {
  wait();
}
```

Correct:

```ts
if (ready) {
  run();
} else {
  wait();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"1tbs"`, `"stroustrup"` or `"allman"` | `"1tbs"` | Block layout policy. |
| `allowSingleLine` | `boolean` | `false` | Allow a whole block on one line. |

`stroustrup` puts subsequent keywords on a new line. `allman` also puts opening braces on their own lines.

```dx
rule brace-style {
  set style "allman"
  set allowSingleLine true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `brace-style:open` | Opening brace placement. |
| `brace-style:body` | First statement placement. |
| `brace-style:close` | Closing brace placement. |
| `brace-style:keyword` | Placement of subsequent block keywords. |

`rule brace-style` turns on every variant; `rule brace-style:body off` turns one off.

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/brace-style) documentation (MIT).
