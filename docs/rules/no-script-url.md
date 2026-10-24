# no-script-url

Reports `javascript:` URLs in strings and template literals. These URLs execute code rather than navigate to a resource.

| | |
| --- | --- |
| Upstream | [`no-script-url`](https://eslint.org/docs/latest/rules/no-script-url) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 10 / 10 corpus cases |

## Enable

```dx
import .ts
rule no-script-url
```

## Examples

Incorrect:

```ts
location.href = "javascript:void(0)";
location.href = `javascript:void(0)`;
```

Correct:

```ts
location.href = "/home";
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-script-url) documentation (MIT).
