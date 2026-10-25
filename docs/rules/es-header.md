# es-header

Require a configured licence comment at the start of a file. The header must match the configured text and have exactly one blank line between it and the code.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule es-header {
  set license "/* Copyright Example */"
}
```

## Examples

With the licence text configured above:

Incorrect:

```ts
const answer = 42;
```

Correct:

```ts
/* Copyright Example */

const answer = 42;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `license` | `string` | `""` | Exact comment text; an empty string disables checks. Use `\n` for multiline text. |
| `skip_files` | `regex` | `""` | File-path pattern exempting files from the header requirement. |

```dx
rule es-header {
  set license "/* Copyright Example */"
  set skip_files /\.generated\.ts$/
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-header:missing` | Missing licence-like first comment in non-empty files. |
| `es-header:invalid` | Licence-like first comment differing from `license`. |
| `es-header:before` | Newlines before the matching header. |
| `es-header:after` | Missing or excess blank lines after the matching header. |

`rule es-header` turns on every variant; `rule es-header:after off` turns one off.

A licence-like comment contains `Copyright`, `@license` or `SPDX-License-Identifier`, ignoring case. Include one of these markers in `license`. The fix preserves the file's newline style.
