# es-func-name-matching

Match a named function expression's name to its variable or property name. Anonymous functions are not required to acquire names by this rule.

| | |
| --- | --- |
| Upstream | [`func-name-matching`](https://eslint.org/docs/latest/rules/func-name-matching) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 193 / 193 corpus cases |

## Enable

```dx
import .ts
rule es-func-name-matching
```

## Examples

Incorrect:

```ts
const foo = function bar() {};
```

Correct:

```ts
const foo = function foo() {};
const bar = function() {};
```

## Options

`mode` is the default option: `rule es-func-name-matching set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` / `"never"` | `"always"` | Require matching names, or forbid matching names. |
| `considerPropertyDescriptor` | `boolean` | `false` | Check functions supplied through property descriptors. |
| `includeCommonJSModuleExports` | `boolean` | `false` | Include CommonJS module export assignments. |

```dx
rule es-func-name-matching {
  set considerPropertyDescriptor true
}
```

## Differences from upstream

- The positional matching policy is named `mode`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/func-name-matching) documentation (MIT).
