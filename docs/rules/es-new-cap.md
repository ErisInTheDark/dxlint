# es-new-cap

Reports lowercase constructor names and capitalised functions called without `new`. Exceptions allow factories and other deliberate naming choices.

| | |
| --- | --- |
| Upstream | [`new-cap`](https://eslint.org/docs/latest/rules/new-cap) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 146 / 146 corpus cases |

## Enable

```dx
import .ts
rule es-new-cap
```

## Examples

Incorrect:

```ts
const first = new person();
const second = Person();
```

Correct:

```ts
const first = new Person();
const second = person();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `capIsNew` | `boolean` | `true` | Capitalised calls require `new`. |
| `newIsCap` | `boolean` | `true` | Constructors must start with a capital letter. |
| `capIsNewExceptions` | `string[]` | `[]` | Capitalised names callable without `new`. |
| `newIsCapExceptions` | `string[]` | `[]` | Lowercase names allowed as constructors. |
| `capIsNewExceptionPattern` | `regex` or `null` | `null` | Pattern for callable capitalised names. |
| `newIsCapExceptionPattern` | `regex` or `null` | `null` | Pattern for constructible lowercase names. |
| `properties` | `boolean` | `true` | Include member calls. |

```dx
rule es-new-cap {
  set capIsNewExceptions ["Factory"]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-new-cap` | Capitalised calls without `new`. |
| `es-new-cap:constructor` | Lowercase constructor names. |

`rule es-new-cap` turns on every variant; `rule es-new-cap:constructor off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/new-cap) documentation (MIT).
