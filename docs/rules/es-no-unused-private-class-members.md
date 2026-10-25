# es-no-unused-private-class-members

Reports private fields and methods whose values are never read, and private accessors that are never accessed. Both JavaScript `#private` names and TypeScript `private` members are checked.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unused-private-class-members`](https://typescript-eslint.io/rules/no-unused-private-class-members) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 160 / 160 corpus cases |

## Enable

```dx
import .ts
rule es-no-unused-private-class-members
```

## Examples

Incorrect:

```ts
class Example {
  private value = 123;
}
```

Correct:

```ts
class Example {
  private value = 123;
  read() {
    return this.value;
  }
}
```

## Differences from upstream

- The upstream TypeScript rule extends ESLint's core check with `private` keyword support. dxlint provides both forms under one rule.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unused-private-class-members) and [ESLint](https://eslint.org/docs/latest/rules/no-unused-private-class-members) documentation (MIT).
