# es-guard-for-in

Require a guard inside for-in loops to filter unwanted properties. For-in includes inherited properties, so checking ownership avoids processing prototype members by mistake.

| | |
| --- | --- |
| Upstream | [`guard-for-in`](https://eslint.org/docs/latest/rules/guard-for-in) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 12 / 12 corpus cases |

## Enable

```dx
import .ts
rule es-guard-for-in
```

## Examples

Incorrect:

```ts
for (const key in object) {
  use(key);
}
```

Correct:

```ts
for (const key in object) {
  if (Object.hasOwn(object, key)) {
    use(key);
  }
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/guard-for-in) documentation (MIT).
