# es-no-dupe-keys

Reports repeated keys in object literals. A later property with the same key can silently replace the earlier value.

| | |
| --- | --- |
| Upstream | [`es-no-dupe-keys`](https://eslint.org/docs/latest/rules/no-dupe-keys) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 50 / 50 corpus cases |

## Enable

```dx
import .ts
rule es-no-dupe-keys
```

## Examples

Incorrect:

```ts
const foo = {
  bar: "baz",
  bar: "qux"
};
```

Correct:

```ts
const foo = {
  bar: "baz",
  quxx: "qux"
};
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-dupe-keys) documentation (MIT).
