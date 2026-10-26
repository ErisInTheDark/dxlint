# es-block-scoped-var

Reports `var` bindings used outside the block where they were declared. Although JavaScript hoists `var` to function scope, keeping its uses inside the declaring block avoids surprising dependencies.

| | |
| --- | --- |
| Upstream | [`block-scoped-var`](https://eslint.org/docs/latest/rules/block-scoped-var) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 106 / 106 corpus cases |

## Enable

```dx
import .ts
rule es-block-scoped-var
```

## Examples

Incorrect:

```ts
function build() {
  if (ready) {
    var result = true;
  }
  console.log(result);
}
```

Correct:

```ts
function build() {
  var result;
  if (ready) {
    result = true;
  }
  console.log(result);
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/block-scoped-var) documentation (MIT).
