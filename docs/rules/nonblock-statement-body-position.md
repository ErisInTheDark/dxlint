# nonblock-statement-body-position

Controls whether a single-statement body without braces sits beside or below its condition. It does not require or forbid braces.

| | |
| --- | --- |
| Upstream | [`@stylistic/nonblock-statement-body-position`](https://eslint.style/rules/nonblock-statement-body-position) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 48 / 48 corpus cases |

## Enable

```dx
import .ts
rule nonblock-statement-body-position
```

## Examples

Incorrect:

```ts
if (foo)
  bar();
```

Correct:

```ts
if (foo) bar();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"beside" \| "below" \| "any"` | `"beside"` | Position of a non-block body. |
| `overrides` | Object with optional `if`, `else`, `while`, `do`, `for` keys | All keys `null` | Override `mode` per statement with `"beside"`, `"below"` or `"any"`. |

```dx
rule nonblock-statement-body-position {
  set mode "below"
  set overrides { while: "beside" }
}
```

## Differences from upstream

- Set the upstream positional string through the named `mode` option.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/nonblock-statement-body-position) documentation (MIT).
