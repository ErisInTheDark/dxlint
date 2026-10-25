# es-prefer-return-this-type

Prefers the `this` return type when a class member returns its own instance. This preserves the derived class type when methods are chained.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-return-this-type`](https://typescript-eslint.io/rules/prefer-return-this-type) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 21 / 21 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-return-this-type
```

## Examples

Incorrect:

```ts
class Foo {
  chain(): Foo {
    return this;
  }
}
```

Correct:

```ts
class Foo {
  chain(): this {
    return this;
  }
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-return-this-type) documentation (MIT).
