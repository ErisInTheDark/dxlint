# es-require-atomic-updates

Reports updates that read a shared value before `await` or `yield` and write it afterwards. Another call can change the value while execution is paused, causing that change to be lost.

| | |
| --- | --- |
| Upstream | [`require-atomic-updates`](https://eslint.org/docs/latest/rules/require-atomic-updates) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 63 / 63 corpus cases |

## Enable

```dx
import .ts
rule es-require-atomic-updates
```

## Examples

Incorrect:

```ts
let total = 0;
async function add() {
  total += await getLength();
}
```

Correct:

```ts
let total = 0;
async function add() {
  const length = await getLength();
  total += length;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowProperties` | `boolean` | `false` | Skip updates to object properties. |

```dx
rule es-require-atomic-updates {
  set allowProperties true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/require-atomic-updates) documentation (MIT).
