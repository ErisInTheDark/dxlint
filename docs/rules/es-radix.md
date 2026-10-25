# es-radix

Requires an explicit radix for `parseInt` and rejects invalid radix values. Naming the base avoids accidentally interpreting a hexadecimal string as decimal, or the reverse.

| | |
| --- | --- |
| Upstream | [`radix`](https://eslint.org/docs/latest/rules/radix) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 91 / 91 corpus cases |

## Enable

```dx
import .ts
rule es-radix
```

## Examples

Incorrect:

```ts
parseInt('071');
parseInt('071', 37);
```

Correct:

```ts
parseInt('071', 10);
parseInt('071', 8);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "as-needed"` | `"always"` | Legacy policy setting; both values require a radix. |

```dx
rule es-radix {
  set mode "always"
}
```

## Differences from upstream

- The deprecated upstream positional option is named `mode`. Like current upstream, both accepted values enforce an explicit radix.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/radix) documentation (MIT).
