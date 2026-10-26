# shared-blank-lines

Limits consecutive blank lines. The default permits two blank lines in a row and removes extras.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule shared-blank-lines
```

This is a language-independent whitespace rule, available through each language's standard library.

## Examples

Incorrect:

```ts
const first = 1;



const second = 2;
```

Correct:

```ts
const first = 1;

const second = 2;
```

## Options

`max` is the default option: `rule shared-blank-lines set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `2` | Maximum blank lines in a matched run. |

```dx
rule shared-blank-lines {
  set max 1
}
```

The matcher starts at runs of at least two blank lines. Setting `max` to zero does not report an isolated single blank line.
