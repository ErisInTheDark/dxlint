# es-unbound-method

Reports methods extracted from their object without binding their receiver. Calling an extracted method can give it the wrong `this`; use a bound function, an arrow function, or an explicit `this: void` annotation when the method does not need a receiver.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/unbound-method`](https://typescript-eslint.io/rules/unbound-method) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 215 / 218 corpus cases |

## Enable

```dx
import .ts
rule es-unbound-method
```

## Examples

Incorrect:

```ts
class Logger {
  log() { console.log(this); }
}
const logger = new Logger();
const log = logger.log;
```

Correct:

```ts
class Logger {
  log() { console.log(this); }
}
const logger = new Logger();
const log = logger.log.bind(logger);
const callback = () => logger.log();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreStatic` | `boolean` | `false` | Allow extracting static methods without binding them. |

```dx
rule es-unbound-method {
  set ignoreStatic true
}
```

## Differences from upstream

- Can miss methods destructured from contextually typed callback parameters or from items in a `for...of` loop.
- Can miss computed method references with numeric keys, such as `foo[1]`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/unbound-method) documentation (MIT).
