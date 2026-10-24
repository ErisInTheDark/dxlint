# no-alert

Reports calls to the global `alert`, `confirm` and `prompt` functions. Use a less intrusive application UI instead, and remove debugging popups.

| | |
| --- | --- |
| Upstream | [`no-alert`](https://eslint.org/docs/latest/rules/no-alert) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule no-alert
```

## Examples

Incorrect:

```ts
alert("Something happened!");
confirm("Are you sure?");
prompt("What's your name?");
```

Correct:

```ts
customAlert("Something happened!");
customConfirm("Are you sure?");
customPrompt("What's your name?");
```

Shadowed local functions are allowed.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-alert) documentation (MIT).
