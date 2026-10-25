# es-no-class-assign

Reports assignments to class declaration bindings. Replacing a class name usually indicates a mistake.

| | |
| --- | --- |
| Upstream | [`es-no-class-assign`](https://eslint.org/docs/latest/rules/no-class-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 18 / 18 corpus cases |

## Enable

```dx
import .ts
rule es-no-class-assign
```

## Examples

Incorrect:

```ts
class A {}
A = 0;
```

Correct:

```ts
class A {
  update(A) {
    A = 0;
  }
}
```

The parameter in the correct example is a separate binding.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-class-assign) documentation (MIT).
