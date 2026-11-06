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

A value counts as disposable when it has a `[Symbol.dispose]` or `[Symbol.asyncDispose]` member; one with both is reported as asynchronous. A value typed `any` is not reported.

`es-use-using-disposable:declare` also leaves out:

- a binding whose value is returned, yielded, passed to `new`, or stored on a member or in an object literal, including through casts, conditionals, arrays, aliases and concise arrow functions, because `using` would dispose a resource that is still live
- iterators whose dispose comes only from the library's `Disposable` or `AsyncDisposable`, such as what `map.values()` returns

Destructuring a disposable value is reported with its own message: `using` can't bind a pattern, and destructuring drops the resource, so hold it first.

`es-use-using-disposable:hold` leaves out an assignment that stores the value on a member or in a variable left out as above. Passing a value to a function still reports: the caller keeps ownership.
