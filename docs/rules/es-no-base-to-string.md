# es-no-base-to-string

Reports string conversions that use an object's unhelpful default representation, such as `"[object Object]"`. Checks include string concatenation, templates, string conversion calls and array joining.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-base-to-string`](https://typescript-eslint.io/rules/no-base-to-string) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 312 / 317 corpus cases |

## Enable

```dx
import .ts
rule es-no-base-to-string
```

## Examples

Incorrect:

```ts
String({});
({}).toString();
[{}].join("");
```

Correct:

```ts
String(42);
JSON.stringify({});
({ toString: () => "Hello!" }).toString();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoredTypeNames` | `string[]` | `["Error", "RegExp", "URL", "URLSearchParams"]` | Types permitted to convert to strings. |
| `checkUnknown` | `boolean` | `false` | Check explicit `unknown` values and unconstrained type parameters. |

```dx
rule es-no-base-to-string {
  set checkUnknown true
}
```

## Differences from upstream

- Recursive unions can hide an object branch that should be reported, such as `type Value = { foo: string } | Value[]`.
- Unresolved or cyclic interface bases can prevent the default string conversion from being identified.
- A mapped `toString` member can be treated as a custom implementation when upstream would report it.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-base-to-string) documentation (MIT).
