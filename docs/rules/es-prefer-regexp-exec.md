# es-prefer-regexp-exec

Prefers `RegExp#exec` to `String#match` when the expression has no global flag. Both forms return the same match information in that case.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-regexp-exec`](https://typescript-eslint.io/rules/prefer-regexp-exec) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 37 / 37 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-regexp-exec
```

## Examples

Incorrect:

```ts
'something'.match(/thing/);
```

Correct:

```ts
/thing/.exec('something');
'some things are just things'.match(/thing/g);
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-regexp-exec) documentation (MIT).
