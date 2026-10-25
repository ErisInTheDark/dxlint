# es-ban-ts-comment

Controls TypeScript directive comments that suppress or enable compiler checks. Defaults allow `@ts-check`, require a description for `@ts-expect-error`, and reject `@ts-ignore` and `@ts-nocheck`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/ban-ts-comment`](https://typescript-eslint.io/rules/ban-ts-comment) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:type-safety`, `rules:recommended` |
| Parity | 114 / 114 corpus cases |

## Enable

```dx
import .ts
rule es-ban-ts-comment
```

## Examples

Incorrect:

```ts
// @ts-ignore
useLegacyValue();
// @ts-expect-error
useLegacyValue();
```

Correct:

```ts
// @ts-expect-error: Legacy declaration has not been updated
useLegacyValue();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `enum(allow, deny, require-description)` | See variants | Directive policy. |
| `descriptionPattern` | `regex` or `null` | `null` | Required description pattern. |
| `minimumDescriptionLength` | `int(0..)` | `3` | Minimum description length. |

Configure each directive through its variant:

```dx
rule es-ban-ts-comment:expect-error {
  set mode "require-description"
  set minimumDescriptionLength 10
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-ban-ts-comment:check` | `@ts-check`; default mode `allow`. |
| `es-ban-ts-comment:expect-error` | `@ts-expect-error`; default mode `require-description`. |
| `es-ban-ts-comment:ignore` | `@ts-ignore`; default mode `deny`. |
| `es-ban-ts-comment:nocheck` | `@ts-nocheck`; default mode `deny`. |

`rule es-ban-ts-comment` turns on every variant; `rule es-ban-ts-comment:check off` turns one off.

## Differences from upstream

- Upstream's per-directive booleans and `allow-with-description` setting become variant-local `mode` values `deny`, `allow` and `require-description`.
- Description settings are variant-local rather than nested in an ESLint options object.
- dxlint does not offer upstream's suggestion to replace `@ts-ignore` with `@ts-expect-error`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/ban-ts-comment) documentation (MIT).
