# no-useless-assignment

Reports assignments whose stored values are never read later. Unlike `no-unused-vars`, this rule targets wasted assignments to variables that otherwise have useful reads.

| | |
| --- | --- |
| Upstream | [`no-useless-assignment`](https://eslint.org/docs/latest/rules/no-useless-assignment) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 116 / 121 corpus cases |

## Enable

```dx
import .ts
rule no-useless-assignment
```

## Examples

Incorrect:

```ts
function run() {
  let value = "used";
  consume(value);
  value = "unused";
}
```

Correct:

```ts
function run() {
  let value = "used";
  consume(value);
  value = "used again";
  consume(value);
}
```

## Differences from upstream

- dxlint does not run ESLint plugins or their `markVariableAsUsed` hooks; four corpus cases require them.
- A value overwritten on every route into `finally` is reported, even when a `catch` could throw before its own write. Upstream keeps such values (`let outcome = "unknown"` before `try { ...; outcome = "success" } catch { outcome = "exception" } finally { use(outcome) }`); dxlint reports the initial value because nothing can read it.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-assignment) documentation (MIT).
