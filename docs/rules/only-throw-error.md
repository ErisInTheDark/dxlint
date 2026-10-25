# only-throw-error

Reports thrown values whose types are not `Error` objects or allowed exceptions. `Error` objects preserve useful information about where a failure started.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/only-throw-error`](https://typescript-eslint.io/rules/only-throw-error) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 87 / 89 corpus cases |

## Enable

```dx
import .ts
rule only-throw-error
```

## Examples

Incorrect:

```ts
throw "error";
throw 0;
```

Correct:

```ts
throw new Error("error");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | Array of strings or type specifier objects | `[]` | Additional allowed types. |
| `allowRethrowing` | `boolean` | `true` | Allow rethrowing catch and promise rejection parameters. |
| `allowThrowingAny` | `boolean` | `true` | Allow values typed as `any`. |
| `allowThrowingUnknown` | `boolean` | `true` | Allow values typed as `unknown`. |

Type specifiers use `{ from: "file", name, path? }`, `{ from: "lib", name }` or `{ from: "package", name, package }`; `name` can be a string or string array.

```dx
rule only-throw-error {
  set allowThrowingAny false
  set allowThrowingUnknown false
}
```

## Differences from upstream

- A class with an unresolved base may incorrectly be accepted even when it does not inherit from `Error`.
- A package-scoped `allow` entry does not match a type declared in an ambient `declare module "name"` block of your own project, so values of that type are reported.
- Upstream extends ESLint's `no-throw-literal` rule with type information.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/only-throw-error) documentation (MIT).
