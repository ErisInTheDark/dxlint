# es-prefer-arrow-callback

Reports function expressions used as callbacks when an arrow function can replace them without changing their meaning. Arrows make callbacks shorter and use lexical `this`.

| | |
| --- | --- |
| Upstream | [`prefer-arrow-callback`](https://eslint.org/docs/latest/rules/prefer-arrow-callback) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 120 / 120 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-arrow-callback
```

## Examples

Incorrect:

```ts
foo(function (a) { return a; });
```

Correct:

```ts
foo(a => a);
foo(function* () { yield 1; });
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowNamedFunctions` | `boolean` | `false` | Allow named callback functions. |
| `allowUnboundThis` | `boolean` | `true` | Allow callbacks using an unbound `this`. |

```dx
rule es-prefer-arrow-callback {
  set allowNamedFunctions true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-arrow-callback) documentation (MIT).
