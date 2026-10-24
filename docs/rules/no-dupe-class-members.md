# no-dupe-class-members

Reports class members with the same name when one would overwrite another. TypeScript method overload declarations are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-dupe-class-members`](https://typescript-eslint.io/rules/no-dupe-class-members) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 99 / 99 corpus cases |

## Enable

```dx
import .ts
rule no-dupe-class-members
```

## Examples

Incorrect:

```ts
class Example {
  method() { return 1; }
  method() { return 2; }
}
```

Correct:

```ts
class Example {
  first() { return 1; }
  second() { return 2; }
}
```

## Differences from upstream

- The upstream TypeScript rule extends ESLint's core rule with overload support.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-dupe-class-members) documentation (MIT).
