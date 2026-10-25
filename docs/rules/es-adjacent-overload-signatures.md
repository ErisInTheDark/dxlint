# es-adjacent-overload-signatures

Requires overload signatures for the same function or method to be next to each other. Keeping overloads together makes the available call forms easier to find.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/adjacent-overload-signatures`](https://typescript-eslint.io/rules/adjacent-overload-signatures) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |
| Parity | 67 / 67 corpus cases |

## Enable

```dx
import .ts
rule es-adjacent-overload-signatures
```

## Examples

Incorrect:

```ts
interface Service {
  run(value: string): void;
  stop(): void;
  run(value: number): void;
}
```

Correct:

```ts
interface Service {
  run(value: string): void;
  run(value: number): void;
  stop(): void;
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/adjacent-overload-signatures) documentation (MIT).
