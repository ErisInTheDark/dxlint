# no-constructor-return

Reports return statements with values inside class constructors. Returning an object can replace the new instance; a bare `return` for early exit is allowed.

| | |
| --- | --- |
| Upstream | [`no-constructor-return`](https://eslint.org/docs/latest/rules/no-constructor-return) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 20 / 20 corpus cases |

## Enable

```dx
import .ts
rule no-constructor-return
```

## Examples

Incorrect:

```ts
class A {
  constructor(a) {
    this.a = a;
    return a;
  }
}
```

Correct:

```ts
class A {
  constructor(a) {
    this.a = a;
  }
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-constructor-return) documentation (MIT).
