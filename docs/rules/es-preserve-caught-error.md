# es-preserve-caught-error

Requires new errors thrown inside a catch block to preserve the caught error as their `cause`. Keeping the original error helps explain what failed before extra context was added.

| | |
| --- | --- |
| Upstream | [`es-preserve-caught-error`](https://eslint.org/docs/latest/rules/preserve-caught-error) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 81 / 81 corpus cases |

## Enable

```dx
import .ts
rule es-preserve-caught-error
```

## Examples

Incorrect:

```ts
try {
  doSomething();
} catch (error) {
  throw new Error('Something went wrong');
}
```

Correct:

```ts
try {
  doSomething();
} catch (error) {
  throw new Error('Something went wrong', { cause: error });
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `requireCatchParameter` | `boolean` | `false` | Require a catch parameter when throwing a new error. |
| `errorClassNames` | `(string \| { name: string; argumentPosition: number })[]` | `[]` | Additional error constructors; `argumentPosition` is the one-based options argument position. A string uses position 2. |

```dx
rule es-preserve-caught-error {
  set requireCatchParameter true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/preserve-caught-error) documentation (MIT).
