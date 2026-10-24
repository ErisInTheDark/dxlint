# prefer-readonly-parameter-types

Requires parameter types to be deeply readonly. Readonly inputs make it clear that a function cannot mutate an array, object, or nested value through its parameter.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-readonly-parameter-types`](https://typescript-eslint.io/rules/prefer-readonly-parameter-types) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 131 / 132 corpus cases |

## Enable

```dx
import .ts
rule prefer-readonly-parameter-types
```

## Examples

Incorrect:

```ts
function array(arg: string[]) {}
function object(arg: { prop: string }) {}
```

Correct:

```ts
function array(arg: readonly string[]) {}
function object(arg: { readonly prop: string }) {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `(string \| type_specifier_file \| type_specifier_lib \| type_specifier_package)[]` | `[]` | Allowed type names and declaration origins. |
| `checkParameterProperties` | `boolean` | `true` | Check constructor parameter properties. |
| `ignoreInferredTypes` | `boolean` | `false` | Check only parameters with written types. |
| `treatMethodsAsReadonly` | `boolean` | `false` | Ignore method writability when judging readonly types. |

```dx
rule prefer-readonly-parameter-types {
  set ignoreInferredTypes true
}
```

## Differences from upstream

- Mutability is not always preserved through generic mapped types over arrays. A mapped `MyType<A>` where `A extends any[]` can be incorrectly treated as readonly.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-readonly-parameter-types) documentation (MIT).
