# es-no-extra-semi

Reports unnecessary semicolons, such as repeated statement terminators or semicolons after function declarations. Removing them makes the code's structure clearer.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-extra-semi`](https://eslint.style/rules/no-extra-semi) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 60 / 60 corpus cases |

## Enable

```dx
import .ts
rule es-no-extra-semi
```

## Examples

Incorrect:

```ts
var x = 5;;
function foo() {
  work();
};
```

Correct:

```ts
var x = 5;
function foo() {
  work();
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-extra-semi) documentation (MIT).
