# no-console

Reports use of `console` methods, including assignments to them. This helps keep debugging output out of shipped browser code; command-line applications may want to leave the rule off.

| | |
| --- | --- |
| Upstream | [`no-console`](https://eslint.org/docs/latest/rules/no-console) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions` |
| Parity | 44 / 44 corpus cases |

## Enable

```dx
import .ts
rule no-console
```

## Examples

Incorrect:

```ts
console.log("Debug message");
console.error("Something happened");
```

Correct:

```ts
applicationLogger.log("Application message");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `string[]` | `[]` | Console method names permitted by the rule. |

```dx
rule no-console {
  set allow ["warn", "error"]
}
```

## Differences from upstream

- dxlint does not offer upstream's suggestions to remove console statements.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-console) documentation (MIT).
