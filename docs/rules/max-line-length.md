# max-line-length

Reports text lines longer than the configured character limit. This language-neutral rule checks raw text; use `max-len` for JavaScript-aware exemptions.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | none |

## Enable

```dx
import .ts
rule max-line-length
```

## Examples

With `max` set to `30`:

Incorrect:

```ts
const message = "a long message that exceeds thirty characters";
```

Correct:

```ts
const message = "short";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(1..)` | `120` | Maximum characters per line. |

```dx
rule max-line-length {
  set max 30
}
```
