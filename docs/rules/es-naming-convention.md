# es-naming-convention

Reports names that do not follow the convention for their declaration kind. Formats, underscores, affixes, and regular expressions can express a shared naming style.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/naming-convention`](https://typescript-eslint.io/rules/naming-convention) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 87 / 89 corpus cases |

## Enable

```dx
import .ts
rule es-naming-convention
```

## Examples

Incorrect:

```ts
const my_value = 1;
class example {}
```

Correct:

```ts
const myValue = 1;
class Example {}
```

## Options

Configure options on the relevant variant. Formats are `camelCase`, `PascalCase`, `strictCamelCase`, `StrictPascalCase`, `snake_case`, and `UPPER_CASE`. Underscore policies are `allow`, `allowDouble`, `allowSingleOrDouble`, `forbid`, `require`, and `requireDouble`.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `formats` | Format enum array | Depends on variant; see below | Allowed formats after trimming permitted underscores and affixes; `[]` skips format checking. |
| `leadingUnderscore` | Underscore enum or `null` | `"allow"` for values/members; `null` for types/imports | Leading underscore policy. |
| `trailingUnderscore` | Underscore enum or `null` | `"allow"` for values/members; `null` for types/imports | Trailing underscore policy. |
| `prefix` | `string[]` | `[]` | Require one of these prefixes when nonempty. |
| `suffix` | `string[]` | `[]` | Require one of these suffixes when nonempty. |
| `customRegex` | `regex` or `null` | `null` | Additional pattern for the processed name. |
| `customMatch` | `boolean` | `true` | Require a pattern match, or forbid one. |

```dx
rule es-naming-convention:variable {
  set formats ["camelCase"]
  set leadingUnderscore "forbid"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-naming-convention:auto-accessor` | Auto-accessor names; `["camelCase"]`. |
| `es-naming-convention:class` | Class names; `["PascalCase"]`. |
| `es-naming-convention:class-method` | Class methods; `["camelCase"]`. |
| `es-naming-convention:class-property` | Class properties; `["camelCase"]`. |
| `es-naming-convention:classic-accessor` | Getter/setter names; `["camelCase"]`. |
| `es-naming-convention:enum` | Enum names; `["PascalCase"]`. |
| `es-naming-convention:enum-member` | Enum members; `["camelCase"]`. |
| `es-naming-convention:function` | Function names; `["camelCase"]`. |
| `es-naming-convention:import` | Import names; `["camelCase", "PascalCase"]`. |
| `es-naming-convention:interface` | Interface names; `["PascalCase"]`. |
| `es-naming-convention:object-method` | Object methods; `["camelCase"]`. |
| `es-naming-convention:object-property` | Object properties; `["camelCase"]`. |
| `es-naming-convention:parameter` | Parameters; `["camelCase"]`. |
| `es-naming-convention:parameter-property` | Constructor parameter properties; `["camelCase"]`. |
| `es-naming-convention:type-alias` | Type aliases; `["PascalCase"]`. |
| `es-naming-convention:type-method` | Type/interface methods; `["camelCase"]`. |
| `es-naming-convention:type-parameter` | Type parameters; `["PascalCase"]`. |
| `es-naming-convention:type-property` | Type/interface properties; `["camelCase"]`. |
| `es-naming-convention:variable` | Variables; `["camelCase", "UPPER_CASE"]`. |

`rule es-naming-convention` turns on every variant; `rule es-naming-convention:object-property off` turns one off.

## Differences from upstream

- Use variants instead of upstream selector arrays. `format` becomes `formats`; `custom.regex` and `custom.match` become `customRegex` and `customMatch`.
- Upstream selector modifiers, type filters, name `filter`, grouped selectors, and selector-priority configuration are not exposed as rule options. The available options do not require types; upstream type-filtered configurations can.
- A named import alias such as `import { "??" as foo } from "foo_bar"` is not assigned the expected import naming context in one corpus case.
- One corpus mismatch comes from the harness checking a typed property key rather than the property's annotated value type. It does not establish a failure in the available naming options.
- Message wording uses the declaration label and the name, with separate reasons for format, underscore, affix, and pattern failures.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/naming-convention) documentation (MIT).
