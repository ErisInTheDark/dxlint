# unified-signatures

Reports overloads that could be combined using a union, an optional parameter or a rest parameter. Review each merge yourself: a combined signature can accept calls that the separate overloads rejected, especially with object types.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/unified-signatures`](https://typescript-eslint.io/rules/unified-signatures) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 115 / 115 corpus cases |

## Enable

```dx
import .ts
rule unified-signatures
```

## Examples

Incorrect:

```ts
declare function convert(value: number): void;
declare function convert(value: string): void;
```

Correct:

```ts
declare function convert(value: number | string): void;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreDifferentlyNamedParameters` | `boolean` | `false` | Skip overloads whose parameter names differ. |
| `ignoreOverloadsWithDifferentJSDoc` | `boolean` | `false` | Skip overloads with different preceding block comments. |

```dx
rule unified-signatures {
  set ignoreDifferentlyNamedParameters true
  set ignoreOverloadsWithDifferentJSDoc true
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/unified-signatures) documentation (MIT).
