# es-no-empty-static-block

Reports empty class static blocks. Remove the block or add a comment explaining why it is intentionally empty.

| | |
| --- | --- |
| Upstream | [`es-no-empty-static-block`](https://eslint.org/docs/latest/rules/no-empty-static-block) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 10 / 10 corpus cases |

## Enable

```dx
import .ts
rule es-no-empty-static-block
```

## Examples

Incorrect:

```ts
class Foo {
  static {}
}
```

Correct:

```ts
class Foo {
  static { bar(); }
}
class Bar {
  static {
    // Intentionally empty.
  }
}
```

## Differences from upstream

- dxlint reports the empty body without the upstream removal suggestion.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-empty-static-block) documentation (MIT).
