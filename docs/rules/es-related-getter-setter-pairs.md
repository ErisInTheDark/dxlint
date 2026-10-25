# es-related-getter-setter-pairs

Requires a getter's return type to be assignable to the corresponding setter's parameter type. This keeps assigning a property back to itself valid.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/related-getter-setter-pairs`](https://typescript-eslint.io/rules/related-getter-setter-pairs) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-related-getter-setter-pairs
```

## Examples

Incorrect:

```ts
interface Box {
  get value(): string;
  set value(newValue: number);
}
```

Correct:

```ts
interface Box {
  get value(): string;
  set value(newValue: string);
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/related-getter-setter-pairs) documentation (MIT).
