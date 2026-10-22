# tsc oracle fixtures

Probe the TS type layer against the real TypeScript compiler. Golden messages are
`checker.typeToString(...)`, so a change to `std/ts/values*.zlint` that silently
changes a type shows up here as a scenario diff.

```sh
ZLINT_LIBRARY=<repo> zlint fixtures fixtures/ts-oracle
node scratch/tools/ts-oracle.mjs            # regenerate expected/diagnostics
node scratch/tools/ts-oracle.mjs --check    # diff tsc against what is on disk
```

`--check` also takes scenario paths. The generator loads TypeScript 6.0.2 from
`scratch/wayward/node_modules`, builds one program per scenario, and writes one
line per expression statement with tsc's printed type. Positions come from the
tsc AST (1-based, end exclusive), matching zlint's report format.

## A scenario

- `.zlint` enables the probe rule for every expression statement:

  ```zlint
  in ts_expression_statement rule ts-oracle {
    warn "value " (ts_value_print_handle(expression.node) ?? "null")
  }
  rule ts-oracle
  ```

- `tsconfig.json` keeps `lib: []` and `types: []` so only hand-declared shapes
  exist. When a standard type is needed, use `lib: ["fixture"]` plus
  `node_modules/typescript/lib/lib.fixture.d.ts` (the generator adds it as a
  declaration input; zlint resolves it as the library).
- `a.ts` holds the probes. One expression statement per probe, one probe per
  concern; small files keep runs cheap. `expected/diagnostics` is generated,
  never hand-edited.

## Known gaps

Measured with a current source build. These scenarios fail on purpose: the
expectation stays tsc's answer until the type layer matches it.

| scenario | probes | gaps | what differs |
|---|---:|---:|---|
| `mapped` | 9 | 4 | mapped types keep `unknown` when the value is a key (`K`), indexed element (`T[K][]`), generic `keyof`, or an `as` remap. |
| `narrowing-typeof` | 17 | 6 | `typeof value` prints the operand type, not the `"string" \| ... \| "function"` union; `typeof x === "object"` adds `& object` and misses `null`/`never`; `switch (typeof ...)` does not narrow. |
| `generics` | 12 | 2 | `Unwrap<number>` stays `unknown` instead of `never`; `keyof Box<number>` inside a generic alias stays `unknown`. |
| `intersections` | 16 | 1 | `T extends ({ p: number } & infer F)` stays `unknown` instead of `0`. |
| `object-keyword` | 9 | 1 | `object` rejects arrays (`IsObject<number[]>` is `0`, tsc says `1`). |
| `printing-cosmetic` | 6 | 6 | print-format only, see below. |
| `enum-object` | 22 | 20 | `keyof` stays `unknown` (direct and through `T[keyof T]`), so `ia`/`ex` stay `unknown`/`any`; enum objects and members print `object`, `number & 0`, `string & "x"` instead of `typeof E`, `E.A`, `S`; a number index on a string or const enum is `unknown`, not `any`. Numeric reverse mapping (`E[0]`, `typeofE[0]` = `string`) matches. |

Passing today: `printing`, `conditional-infer`, `awaited`.

A default `zlint fixtures` run includes this family, so the seven known-gap
scenarios above report as failures there too until they are fixed.

`printing-cosmetic` isolates print differences so the other scenarios can pass:

- function types print as `object` (plain, generic, and inferred method callbacks)
- an alias reference prints expanded (`{ id: number; }` instead of `Named`)
- `[string, number?]` instead of `[string, (number | undefined)?]`
- `[string, number]` instead of `[string, ...number[]]`

## Runner note

`target/bin/c6c7f1bd-train-2026-10-22t02-19-18-621z.exe` predates fixes in the
working tree, so it shows extra gaps (`awaited`, most `infer`-under-`&`
answers). Regenerate gaps and counts against a current build.
