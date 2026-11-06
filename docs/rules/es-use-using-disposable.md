# es-use-using-disposable

Reports disposable values stored or discarded without `using` or `await using`. A resource nothing disposes of stays open until something else happens to clean it up.

| | |
| --- | --- |
| Upstream | none |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | not measured |

## Enable

```dx
import .ts
rule es-use-using-disposable*
```

## Examples

Incorrect:

```ts
declare function open(): Disposable;
declare function openAsync(): AsyncDisposable;

const file = open();
const remote = openAsync();
open();
```

Correct:

```ts
declare function open(): Disposable;
declare function openAsync(): AsyncDisposable;

using file = open();
await using remote = openAsync();
void open();
```

## Variants

| Rule | Reports |
| --- | --- |
| `es-use-using-disposable:declare` | a `const`, `let` or `var` initialized with a disposable value, on its name |
| `es-use-using-disposable:hold` | an expression statement whose value is disposable |

A value counts as disposable when it has a `[Symbol.dispose]` or `[Symbol.asyncDispose]` member; one with both is reported as asynchronous. This includes library iterators under `lib: ["esnext"]`, such as what `map.values()` returns. A value typed `any` is not reported.
