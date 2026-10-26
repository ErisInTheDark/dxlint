# es-no-lone-blocks

Reports standalone blocks that provide no useful scope. Blocks containing lexical declarations can be needed and are not treated as redundant.

| | |
| --- | --- |
| Upstream | [`no-lone-blocks`](https://eslint.org/docs/latest/rules/no-lone-blocks) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 50 / 50 corpus cases |

## Enable

```dx
import .ts
rule es-no-lone-blocks
```

## Examples

Incorrect:

```ts
function update() {
  {
    run();
  }
}
```

Correct:

```ts
function update() {
  run();
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-lone-blocks) documentation (MIT).
