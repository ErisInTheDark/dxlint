# no-async-promise-executor

Reports async functions passed as `Promise` executors. Errors thrown by an async executor do not reject the promise created by the constructor.

| | |
| --- | --- |
| Upstream | [`no-async-promise-executor`](https://eslint.org/docs/latest/rules/no-async-promise-executor) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule no-async-promise-executor
```

## Examples

Incorrect:

```ts
const result = new Promise(async resolve => {
  resolve(await foo);
});
```

Correct:

```ts
const result = Promise.resolve(foo);
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-async-promise-executor) documentation (MIT).
