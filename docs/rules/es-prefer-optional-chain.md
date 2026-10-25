# es-prefer-optional-chain

Reports repeated nullish guards that can use optional chaining. `?.` keeps property and call chains short while stopping at `null` or `undefined`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-optional-chain`](https://typescript-eslint.io/rules/prefer-optional-chain) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 1091 / 1104 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-optional-chain
```

## Examples

Incorrect:

```ts
declare const foo: { bar: string } | undefined;
foo && foo.bar;
```

Correct:

```ts
declare const foo: { bar: string } | undefined;
foo?.bar;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `checkAny` | `boolean` | `true` | Check `any` values. |
| `checkUnknown` | `boolean` | `true` | Check `unknown` values. |
| `checkString` | `boolean` | `true` | Check string values. |
| `checkNumber` | `boolean` | `true` | Check number values. |
| `checkBoolean` | `boolean` | `true` | Check boolean values. |
| `checkBigInt` | `boolean` | `true` | Check bigint values. |
| `requireNullish` | `boolean` | `false` | Require a nullable checked operand; also available on `empty-object`. |
| `allowPotentiallyUnsafeFixesThatModifyTheReturnTypeIKnowWhatImDoing` | `boolean` | `false` | Allow fixes that may add `undefined` to the return type. |

```dx
rule es-prefer-optional-chain {
  set requireNullish true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-prefer-optional-chain` | Logical guard chains. |
| `es-prefer-optional-chain:empty-object` | Empty-object fallbacks such as `(foo || {}).bar`. |

`rule es-prefer-optional-chain` turns on every variant; `rule es-prefer-optional-chain:empty-object off` turns one off.

## Differences from upstream

- dxlint applies some edits as autofixes where upstream offers the same text only as a suggestion. This accounts for 12 corpus differences involving repeated calls, `new.target`, and `typeof globalThis` guards. Review these edits because removing repeated evaluation can matter.
- One corpus case uses `yield` outside a generator, which dxlint's parser does not accept.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-optional-chain) documentation (MIT).
