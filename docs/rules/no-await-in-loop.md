# no-await-in-loop

Reports `await` operations inside loops, including `await using`. Independent operations can often start together and be awaited outside the loop.

| | |
| --- | --- |
| Upstream | [`no-await-in-loop`](https://eslint.org/docs/latest/rules/no-await-in-loop) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 37 / 37 corpus cases |

## Enable

```dx
import .ts
rule no-await-in-loop
```

## Examples

Incorrect:

```ts
async function process(things) {
  for (const thing of things) {
    await doAsyncWork(thing);
  }
}
```

Correct:

```ts
async function process(things) {
  const promises = [];
  for (const thing of things) {
    promises.push(doAsyncWork(thing));
  }
  await Promise.all(promises);
}
```

Do not parallelise work that depends on earlier iterations, needs retries, or must respect resource limits. Disable the rule where sequential work is intentional.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-await-in-loop) documentation (MIT).
