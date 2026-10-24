# prefer-string-starts-ends-with

Reports string boundary checks that can use `startsWith` or `endsWith`. These methods show the intent more directly than index, slice, or regular expression checks.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-string-starts-ends-with`](https://typescript-eslint.io/rules/prefer-string-starts-ends-with) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:types` |
| Parity | 133 / 133 corpus cases |

## Enable

```dx
import .ts
rule prefer-string-starts-ends-with
```

## Examples

Incorrect:

```ts
declare const text: string;
text.indexOf('bar') === 0;
/bar$/.test(text);
```

Correct:

```ts
declare const text: string;
text.startsWith('bar');
text.endsWith('bar');
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowSingleElementEquality` | `"never" \| "always"` | `"never"` | Allow first- or last-character equality checks when set to `"always"`. |

```dx
rule prefer-string-starts-ends-with {
  set allowSingleElementEquality "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `prefer-string-starts-ends-with` | Character and string-method comparisons. |
| `prefer-string-starts-ends-with:regex` | Anchored regular expression tests. |

`rule prefer-string-starts-ends-with` turns on every variant; `rule prefer-string-starts-ends-with:regex off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-string-starts-ends-with) documentation (MIT).
