# one-var

Controls whether variable declarations are combined or separated within a scope. By default, declarations of the same kind are combined.

| | |
| --- | --- |
| Upstream | [`one-var`](https://eslint.org/docs/latest/rules/one-var) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 296 / 296 corpus cases |

## Enable

```dx
import .ts
rule one-var
```

## Examples

Incorrect:

```ts
function foo() {
  let bar;
  let baz;
}
```

Correct:

```ts
function foo() {
  let bar, baz;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of grouping modes or policy objects | `[]` | Upstream-style grouping policy; an empty array uses `"always"`. |

Modes are `"always"`, `"never"` and `"consecutive"`. `"consecutive"` combines only adjacent declarations. Policy objects accept `var`, `let`, `const`, `using`, `awaitUsing`, `initialized` and `uninitialized` with those modes, plus `separateRequires: boolean`.

```dx
rule one-var {
  set options ["never"]
}
```

## Differences from upstream

- Put upstream positional configuration into the named `options` array.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/one-var) documentation (MIT).
