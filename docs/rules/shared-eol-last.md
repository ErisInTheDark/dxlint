# shared-eol-last

Require a final newline in non-empty files, or forbid final newlines. This does not limit the number of final newlines in the default mode.

| | |
| --- | --- |
| Upstream | [`@stylistic/eol-last`](https://eslint.style/rules/eol-last) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 40 / 44 corpus cases |

## Enable

```dx
import .ts
rule shared-eol-last
```

## Examples

Incorrect (no newline after the final semicolon):

```ts
const answer = 42;
```

Correct (a newline follows the final semicolon):

```ts
const answer = 42;

```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` / `"never"` | `"always"` | Require or forbid trailing newlines. |

```dx
rule shared-eol-last {
  set mode "never"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `shared-eol-last` | Missing final newline in `always` mode. |
| `shared-eol-last:never` | Final newlines in `never` mode. |

`rule shared-eol-last` turns on every variant; `rule shared-eol-last:never off` turns one off.

## Differences from upstream

- The positional policy is named `mode`.
- Four corpus cases are unsupported by the test harness because their source is labelled binary. They contain ordinary text; this is a fixture mapping gap, not an established rule defect.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/eol-last) documentation (MIT).
