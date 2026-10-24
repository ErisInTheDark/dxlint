# tab-indent

Reports tabs in leading indentation and replaces each tab with a fixed number of spaces. This text-level rule applies to every declared language, not just TypeScript.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule tab-indent
```

Any language import brings in the shared whitespace rules.

## Examples

The incorrect example begins with a literal tab.

Incorrect:

```ts
	run();
```

Correct:

```ts
    run();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `width` | integer, at least 1 | `4` | Spaces substituted for each indentation tab. |

```dx
rule tab-indent {
  set width 2
}
```

The replacement is per tab, not alignment to tab stops. Tabs after non-whitespace text are not checked.
