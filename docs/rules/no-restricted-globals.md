# no-restricted-globals

Reports uses of configured global names. Local variables with the same name are allowed, and type-only references are ignored.

| | |
| --- | --- |
| Upstream | [`no-restricted-globals`](https://eslint.org/docs/latest/rules/no-restricted-globals) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 150 / 150 corpus cases |

## Enable

```dx
import .ts
rule no-restricted-globals
```

## Examples

With `globals` set to `["event"]` and `event` available as a global:

Incorrect:

```ts
function onClick() {
  console.log(event);
}
```

Correct:

```ts
function onClick(event: Event) {
  console.log(event);
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `globals` | `(restriction_global \| string)[]` | `[]` | Forbidden names, or records with `name` and optional `message`. |
| `checkGlobalObject` | `boolean` | `false` | Also check properties on global objects. |
| `globalObjects` | `string[]` | `[]` | Extra global-object names in addition to `globalThis`, `self` and `window`. |

```dx
rule no-restricted-globals {
  set globals [{ name: "event", message: "Use the local event parameter." }]
  set checkGlobalObject true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-restricted-globals` | Unshadowed global identifiers. |
| `no-restricted-globals:member` | Global-object property access when enabled. |

`rule no-restricted-globals` turns on every variant; `rule no-restricted-globals:member off` turns one off.

## Differences from upstream

- Put restrictions in `globals`, rather than ESLint's positional option list.
- Diagnostics say `Unexpected use of 'name'.`, followed by any custom message.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-restricted-globals) documentation (MIT).
