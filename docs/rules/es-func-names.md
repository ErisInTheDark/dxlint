# es-func-names

Require or forbid names on function expressions. Names can make stack traces easier to read; the `as-needed` policy accepts names inferred from surrounding code.

| | |
| --- | --- |
| Upstream | [`es-func-names`](https://eslint.org/docs/latest/rules/func-names) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 109 / 109 corpus cases |

## Enable

```dx
import .ts
rule es-func-names
```

## Examples

Incorrect:

```ts
Foo.prototype.bar = function() {};
```

Correct:

```ts
Foo.prototype.bar = function bar() {};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` / `"as-needed"` / `"never"` | `"always"` | Require names, require only names not inferred automatically, or forbid unnecessary names. |
| `generators` | `"always"` / `"as-needed"` / `"never"` / `null` | `null` | Override the policy for generators; null inherits `mode`. |

```dx
rule es-func-names {
  set mode "as-needed"
}
```

## Differences from upstream

- The positional naming policy is named `mode`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/func-names) documentation (MIT).
