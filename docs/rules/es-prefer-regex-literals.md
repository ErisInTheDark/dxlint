# es-prefer-regex-literals

Reports `RegExp` constructors with fixed patterns and flags. Regex literals avoid the extra layer of string escaping; constructors remain useful for dynamic patterns.

| | |
| --- | --- |
| Upstream | [`es-prefer-regex-literals`](https://eslint.org/docs/latest/rules/prefer-regex-literals) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 251 / 251 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-regex-literals
```

## Examples

Incorrect:

```ts
new RegExp('abc');
RegExp('abc', 'u');
```

Correct:

```ts
/abc/;
/abc/u;
new RegExp(pattern);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `disallowRedundantWrapping` | `boolean` | `false` | Also report constructors wrapping regex literals. |

```dx
rule es-prefer-regex-literals {
  set disallowRedundantWrapping true
}
```

## Differences from upstream

- dxlint reports these cases without upstream's suggested edits.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-regex-literals) documentation (MIT).
