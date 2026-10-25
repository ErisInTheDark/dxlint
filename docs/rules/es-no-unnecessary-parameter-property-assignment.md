# es-no-unnecessary-parameter-property-assignment

Reports assignments that repeat a constructor parameter property's automatic assignment. Parameter properties already create and initialise the corresponding member.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-parameter-property-assignment`](https://typescript-eslint.io/rules/no-unnecessary-parameter-property-assignment) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 45 / 45 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-parameter-property-assignment
```

## Examples

Incorrect:

```ts
class Foo {
  constructor(public bar: string) {
    this.bar = bar;
  }
}
```

Correct:

```ts
class Foo {
  constructor(public bar: string) {}
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-parameter-property-assignment) documentation (MIT).
