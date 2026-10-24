# no-mixed-enums

Reports enums containing both numeric and string members. Keeping one value kind makes enum use and iteration easier to understand.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-mixed-enums`](https://typescript-eslint.io/rules/no-mixed-enums) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 64 / 64 corpus cases |

## Enable

```dx
import .ts
rule no-mixed-enums
```

## Examples

Incorrect:

```ts
enum Status {
  Unknown,
  Open = "open",
}
```

Correct:

```ts
enum Status {
  Unknown = "unknown",
  Open = "open",
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-mixed-enums) documentation (MIT).
