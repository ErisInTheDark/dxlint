# es-id-denylist

Reports identifiers on a list of forbidden names. Use it to avoid vague names while still allowing calls to existing APIs and reads of their properties.

| | |
| --- | --- |
| Upstream | [`es-id-denylist`](https://eslint.org/docs/latest/rules/id-denylist) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 145 / 145 corpus cases |

## Enable

```dx
import .ts
rule es-id-denylist
```

## Examples

With `names` set to `["data", "callback"]`:

Incorrect:

```ts
const data = {};
function callback() {}
```

Correct:

```ts
const encodingOptions = {};
function processFileResult() {}
callback();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `names` | `string[]` | `[]` | Forbidden identifiers; an empty list reports nothing. |

```dx
rule es-id-denylist {
  set names ["data", "callback"]
}
```

## Differences from upstream

- Put restricted names in `names`, rather than additional ESLint positional options.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/id-denylist) documentation (MIT).
