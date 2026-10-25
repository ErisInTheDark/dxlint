# es-non-nullable-type-assertion-style

Reports type assertions that only remove `null` and `undefined`. A non-null assertion (`!`) is shorter and does not repeat a type that may later change.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/non-nullable-type-assertion-style`](https://typescript-eslint.io/rules/non-nullable-type-assertion-style) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 22 / 26 corpus cases |

## Enable

```dx
import .ts
rule es-non-nullable-type-assertion-style
```

## Examples

Incorrect:

```ts
declare const maybe: string | undefined;
const definitely = maybe as string;
```

Correct:

```ts
declare const maybe: string | undefined;
const definitely = maybe!;
```

## Differences from upstream

- Assertions to aliases such as `NonNullable<T>` are not always recognised as removing only nullish values.
- The nullable type of an awaited promise result is not always preserved, so an assertion such as `(await nullablePromise()) as string` may be missed.
- Generic `ArrayLike<T>` indexed values may lose their possible `undefined` type, so an assertion such as `array[0] as T` may be missed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/non-nullable-type-assertion-style) documentation (MIT).
