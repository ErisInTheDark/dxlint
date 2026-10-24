# switch-exhaustiveness-check

Reports missing cases when switching over a union of literal types or an enum. This helps catch forgotten branches when the type gains a new member.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/switch-exhaustiveness-check`](https://typescript-eslint.io/rules/switch-exhaustiveness-check) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 113 / 113 corpus cases |

## Enable

```dx
import .ts
rule switch-exhaustiveness-check
```

## Examples

Incorrect:

```ts
declare const day: "Monday" | "Tuesday";
switch (day) {
  case "Monday": break;
}
```

Correct:

```ts
declare const day: "Monday" | "Tuesday";
switch (day) {
  case "Monday": break;
  case "Tuesday": break;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowDefaultCaseForExhaustiveSwitch` | boolean | `true` | Allow a default branch even when all union members are covered. |
| `considerDefaultExhaustiveForUnions` | boolean | `false` | Let a default branch cover missing union members. |
| `requireDefaultForNonUnion` | boolean | `false` | Require a default branch for open-ended types such as `number`. |
| `defaultCaseCommentPattern` | `regex \| null` | `null` | Pattern for a comment marking an intentionally omitted default. |

```dx
rule switch-exhaustiveness-check {
  set requireDefaultForNonUnion true
  set defaultCaseCommentPattern /skip default/
}
```

## Differences from upstream

- `defaultCaseCommentPattern` is a dxlint regex, not an ESLint string containing a regex.
- Upstream can suggest missing branches; dxlint reports them without a suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/switch-exhaustiveness-check) documentation (MIT).
