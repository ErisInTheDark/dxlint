# no-misused-spread

Reports spreads whose types suggest a likely mistake, such as promises, functions, maps, arrays, or class instances copied into objects. It also checks strings spread into arrays or call arguments, where complex characters can be split.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-misused-spread`](https://typescript-eslint.io/rules/no-misused-spread) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 129 / 134 corpus cases |

## Enable

```dx
import .ts
rule no-misused-spread
```

## Examples

Incorrect:

```ts
declare function getObject(): Record<string, string>;
const copy = { ...getObject };
declare const map: Map<string, number>;
const mapCopy = { ...map };
```

Correct:

```ts
declare function getObject(): Record<string, string>;
const copy = { ...getObject() };
declare const map: Map<string, number>;
const mapCopy = Object.fromEntries(map);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `(string \| type_specifier_file \| type_specifier_lib \| type_specifier_package)[]` | `[]` | Types explicitly permitted in spreads. Specifiers identify a name from a file, library, or package. |

```dx
rule no-misused-spread {
  set allow ["BrandedString"]
}
```

## Differences from upstream

- Array unions and intersections can be missed, for example spreading `number[] | string[]` into an object.
- Map types inferred from constructor arguments containing pair arrays can be missed.
- Iterable types exported by ambient modules can lose their symbol-member or origin information, affecting detection and package-based exemptions.
- The string diagnostic recommends `Intl.Segmenter` and explains code-point versus UTF-16 splitting; wording differs from upstream.
- Upstream suggestions are not supplied.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-misused-spread) documentation (MIT).
