# no-explicit-any

Reports explicit `any` types, which bypass type checking. Prefer a precise type, or `unknown` when the value's shape is not known.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-explicit-any`](https://typescript-eslint.io/rules/no-explicit-any) |
| Needs types | no |
| Fix | autofix (with `fixToUnknown`) |
| Rulesets | none |
| Parity | 134 / 134 corpus cases |

## Enable

```dx
import .ts
rule no-explicit-any
```

## Examples

Incorrect:

```ts
const age: any = "seventeen";
const ages: any[] = ["seventeen"];
```

Correct:

```ts
const age: string = "seventeen";
const ages: string[] = ["seventeen"];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `fixToUnknown` | `boolean` | `false` | Automatically replace explicit `any` with a safer type. |
| `ignoreRestArgs` | `boolean` | `false` | Allow `any` in rest parameter arrays. |

```dx
rule no-explicit-any {
  set fixToUnknown true
  set ignoreRestArgs true
}
```

## Differences from upstream

- dxlint does not offer the upstream opt-in suggestions. Automatic replacement requires `fixToUnknown`.
- The fixer normally uses `unknown`, but uses `PropertyKey` for `keyof any` and `unknown[]` for a bare rest parameter annotation. Replacements can require further type corrections.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-explicit-any) documentation (MIT).
