# class-methods-use-this

Reports instance methods and function-valued fields that never use `this`. Such functions may belong outside the instance or be static methods.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/class-methods-use-this`](https://typescript-eslint.io/rules/class-methods-use-this) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 283 / 283 corpus cases |

## Enable

```dx
import .ts
rule class-methods-use-this
```

## Examples

Incorrect:

```ts
class Example {
  answer() { return 42; }
}
```

Correct:

```ts
class Example {
  answer() { return this.value; }
  static constant() { return 42; }
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `exceptMethods` | `string[]` | `[]` | Exempt method names. |
| `enforceForClassFields` | `boolean` | `true` | Check function-valued fields. |
| `ignoreOverrideMethods` | `boolean` | `false` | Exempt members marked `override`. |
| `ignoreClassesWithImplements` | `"all"`, `"public-fields"` or `null` | `null` | Exempt all or public members of implementing classes. |
| `ignoreClassesThatImplementAnInterface` | `boolean` or `"public-fields"` | `false` | TypeScript upstream spelling of the interface exemption. |

```dx
rule class-methods-use-this {
  set ignoreOverrideMethods true
  set ignoreClassesThatImplementAnInterface "public-fields"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `class-methods-use-this` | Instance methods. |
| `class-methods-use-this:field` | Function-valued instance fields. |

`rule class-methods-use-this` turns on every variant; `rule class-methods-use-this:field off` turns one off.

## Differences from upstream

- This covers the core rule and the TypeScript extension, including interface and override exceptions.
- `ignoreClassesWithImplements` is an additional dxlint spelling; `"all"` corresponds to `true` in `ignoreClassesThatImplementAnInterface`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/class-methods-use-this) documentation (MIT).
