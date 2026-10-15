# Type layers

Status: design, not implemented.

Goal: a language's names, types and control effects are declared in
`.zlint`, next to the nodes they belong to, and answered by the engine at
native speed. The declarations are not a programming language: no mutable
state, no loops, no user-written recursion budgets, no classes. The engine
owns every algorithm (resolution, unwrapping, narrowing, propagation,
budgets, caching); a language only says how its nodes fit in.

This replaces rule-shaped native analysis (today's `bind.rs`, most of
`types/`, the typed `topics/`) with declarations any language can write.
Parsing context-sensitive syntax stays native (see AGENTS.md).

## Items

Seven items, written in a node's own block (`match x ... { }`, or
`in ts_kind { }` for native kinds), like `let` and `indent`:

| item | meaning |
|---|---|
| `scope <class>, ...` | the node opens a scope of these classes; several `scope` items nest, outermost first |
| `declares <class>, ... <name> in nearest <scope class>, ... [hoisted]` | the node declares `name` (a field) into the nearest enclosing scope of one of those classes; a text (`"arguments"`) declares that name implicitly, the node itself its declaration, yielding to explicit ones |
| `in <declaration kind> { merges <class>, ... }` (module item, may be conditional) | declarations of that kind in those classes merge across files (augmentations, `lib` interfaces); only they consult merged scopes and program inputs, other direct publications answer from their own module; an undecidable condition is unknown |
| `declares ... <name> [keyed <expr>] [where <expr>] in ...` | per target after the path expands (`node` = declaring node, `it` = target; implicit names use the declaring node as `it`): `keyed` gives the target's key, `where` must be boolean and `false` skips that target; an error or non-boolean skips it and reports one engine warning |
| `resolves <class>` | the node's text uses a name; the engine looks it up |
| `resolves <class> ... outside <node expr>` | same lookup start and nested locals, but skips the scopes opened by the selected local ancestor (`node` = item owner, `it` = reference), e.g. parameter decorators resolving outside their method; a null, non-ancestor, scope-less or failing expression is a Names failure (no fallback to ordinary lookup); `resolve`, visibility and `name_scopes(reference)` share it |
| `targets <field> ?? <field>, ...` | a declaration, use or effect aimed at this node (a pattern, a destructuring literal, a qualified name) passes on to these fields; per group the first that reaches anything |

Field paths are field names joined by `.`. Special steps (a field of the same name wins): `parent` (the node's parent: `declares value parent.parameter`), `it` (the node), `through` (the reference it was reached through), `definition` (the declaration a name resolves to), `position` (the node's index among its parent's nodes in the same field).
| `unwraps <value>` / `unwraps <effect> ...` / `unwraps { ... }` | what the node stands for, and what evaluating it does |
| `fits <trait>, ... [when <expr>]` | the node directly has these traits |
| `in <field> propagates <x>` | `x` applies throughout that field's subtree (narrowing tests, and any other context) |

`when <expr> { items }` makes any group of items conditional:

```zlint
in ts_logical {
  when operator == "&&" { unwraps { left | right; truthy left & right } }
  when operator == "||" { unwraps { left | right; truthy left | right } }
}
```

Classes, traits and user effects are never listed separately: they exist
because an item names them. A class or trait that is asked for but never
declared or fitted is a config error, as is an `in nearest` scope class no
node opens.

## Scopes and names

```zlint
lang rust "*.rs" {
  scope module
  match rs_function /\bfn\s+/ name:identifier :rs_parameters body:braces {
    scope function
    declares value name in nearest module, block hoisted
  }
  match rs_let /\blet\s+/ pattern:rs_pattern :rs_annotation? init:rs_expression? {
    declares value pattern.binding in nearest block
  }
  match rs_path_expression path:identifier {
    resolves value
    unwraps definition
  }
}
```

- **Classes are namespaces.** A name declared into `value` is only found by
  `resolves value`. One node may declare into several (`declares value, type
  name` for a TS class).
- **Shadowing is the default.** A declaration is visible from its position
  on; a later declaration of the same name in the same scope takes over from
  its position. `hoisted` makes it visible in the whole scope (TS `var`,
  `function`, `class`, `let`/`const`; Rust `fn` and items).
- **Resolution** walks out from the using node through enclosing scopes to
  the first visible declaration of the class. The result fills the existing
  links and facts: `declaration`, `references`, `resolved` (`local`,
  `import`, `global` when none), `uses`.
- **Imports** link scopes across files:
  `in ts_import_specifier { resolves value imported in module source }`.
  Turning a specifier (`"./foo"`) into a file stays a front-end question
  (`Frontend`), since resolution rules (tsconfig paths, packages) are
  hostile and language-specific; reads go through `engine/reads.rs`.

## Unwrapping: values

A node unwraps into what it stands for. The engine keeps unwrapping until it
reaches nodes that `fits` traits. Types are nodes: an annotation, interface,
struct, signature or literal type is the type; nothing is constructed.

| form | meaning |
|---|---|
| `unwraps x` | stands for `x` (a field, link or expression giving a node) |
| `unwraps a ?? b` | the first that exists |
| `unwraps a \| b` | one of them (could be either) |
| `unwraps a & b` | both at once (TS intersections) |
| `unwraps \| ...types` / `unwraps & ...types` | spread a list with that operator |
| `unwraps from x at key` / `unwraps x.[key]` | the member `key` of what `x` unwraps to, found in that node's scope; `at 0` / `.[0]` for positions |
| `unwraps !x` | not `x` (truth unwraps only) |
| `unwraps x \ t, ...` | `x` without values of these traits (`excludes t, ... from x`); tightest after `.[k]` and `!`, then `&`, `\|`, `??` |
| `excludes t, ... [from x]` / `includes v [from x]` / `has key [from x]` | under `truthy`: `x`'s alternatives with none of these traits (each excludes alone) / that `v` could be (`===`, `instanceof`) / with that member (`in`) |
| `unwraps maybe <effect> ...` | the effect may happen, or the node completes normally (calls that may throw) |
| `through` | in a node reached by unwrapping a reference, that reference |
| `trait t, ...` | a closed, nodeless value with exactly the listed traits; no identity, syntax or declaration (`trait void, undefined` implies nothing else) |
| `returned default v` | the declared valued returns, unioned with `v` for bare returns or normal completion; no returns and no normal completion is never |
| `carries [<ordered argument path> [using v]] as <description>` (per kind) | the nominal carrier a value instantiates (generic declaration + its ordered arguments + a language description for display/policy); without a path it has zero arguments (enums); rules compare carriers, not assignability |
| `select <check> against <pattern> then <yes> else <no> [distribute <sites>] [dynamic both]` (value recipe) | conditional value: the shared matcher proves yes/no and binds the owner's `generics` (infer slots; unbound = known absent); undecidable is unknown, never a guessed union. `distribute` splits the check's whole DNF arms (empty union = never) and rebinds the naked formal per arm. `dynamic both` (with a `when check always any` guard): infer first; a top-bound pattern picks yes only, otherwise yes \| no |
| `apparent <trait> inherits <value>` (language) | trait-only values with the trait (primitives) read members and closedness through `value` (`apparent number inherits instance "Number"`); never their own members, traits or identity; an unresolved `value` leaves them unknown |
| `product <shape> <items> fits <traits> as <constant record> [inherits v] [readonly inherits v] [optional v]` (language) | an ordered product shape (tuples, rest packs): traits, carrier description, member base (readonly products use `readonly inherits`) and the optional-element payload |
| `unwraps product <shape> of <sites> [using v] [optional <path>] [repeated <path>] [repeated values <path>] [readonly <path>]` | the node is a product: one element per site (real values and frames, `using` per element), optional/repeated read at each site, readonly at the node; a missing flag is false, an unreadable one unknown. `repeated` marks a spread collection (its ordinal members repeat there); `repeated values` marks an element whose own value repeats (array literals and `T[]`). A missing global base (no library) is no inherited layer |
| `pack <shape>` (on a rest parameter kind) | rest arguments are captured as an ordered product of that shape |
| `constrains <value>` (on a generic formal) | the formal's bound, evaluated in the bound frame (so dependent bounds see real arguments); inferred candidates violating a known bound fall back to it, explicit ones reject the candidate |
| `defines <value>` (on a syntax owner) | the raw definition sites the node selects (qualified names: the member, not the left side); without it, name bindings decide. A closed scope proving the member absent yields no sites so `??` falls through; incomplete selection stays unknown; selected frames, files and classes are kept |
| `unwraps <value> for <declaration classes>` | class-specific view of a dual declaration (enum type vs runtime object) used during real definition/instance binding; qualified first, default `unwraps` as fallback; ordered arguments, file, `through` and frame kept |
| `arguments <path>` (per kind) | the ordered type arguments a reference supplies, read in the caller's frame |
| `top <trait>` / `dynamic <trait>` (language) | `top` (TS `unknown`): universal target, meet identity, absorbs joins; `dynamic` (TS `any`): the same plus universal source, and wins joins over `top`. An ordinary unknown answer stays epistemic, never `top` |
| `indexed [repeated] <path> [<strings>] [using v] [in views]`, `...<path> [using v]`, `&...<path> [using v]` | per-slot / spread recipes evaluated lazily at each real source site and frame, keeping order and unknown slots |
| `keys text\|ordinal\|symbol <traits>` (language) | which traits a key value must have to belong to each key domain |
| `indexed text\|ordinal\|symbol <path> [using v] [in views]` | an index signature: an unbounded, lookup-only member inventory for that key domain |
| `indexed keyed <path> by <key value> [using v] [in views]` | computed members with finite keys evaluated per site (Known keys cached per canonical leaf and path, keeping the frame) |
| `fits T, ... [in views] open` | the listed traits are known; traits not listed are uncertain (not false) for that view (`any`, `unknown`); `when` and scope views apply as for `fits` |
| `members <views> closed [inherits v]` | the scope's named-member list is complete (presence proofs may drop absent keys); every active base view must be closed too, and unresolved members or heritage never prove closed |
| `flag <name> [when c]` (per kind) | a named fact about a declaration (`readonly`, `method`, `private`...), reported on member sources by `value_children`; a flag whose condition is unknown makes the site's flags unknown unless another item of the same name holds |
| `widens v` (per kind) | the recipe `.widened` uses; without one, `.widened` is the normal value, keeping union/intersection structure |

Precedence follows TS: `a | b & c` is `a | (b & c)`.

```zlint
in ts_declarator { unwraps annotation ?? init }
in ts_identifier { unwraps definition }
in ts_member { unwraps from object at property }
in ts_call { unwraps callee.result }
in ts_conditional { unwraps consequent | alternate }
in ts_union_type { unwraps | ...types }
in ts_intersection_type { unwraps & ...types }
in ts_object_pattern_property { unwraps from parent.source at key }
in ts_type_parameter { unwraps from through.arguments at index ?? default }
```

**Generics** need no substitution item. In `interface Box<T> { value: T }`
reached through `let b: Box<string | undefined>`, `b.value` unwraps to the
member's annotation `T`, which resolves to the type parameter, which unwraps
`from through.arguments at index`: `through` is `Box<string | undefined>`,
so `T` stands for `string | undefined` there and for something else through
another reference.

**Named library types** (`Promise`, `Array`) resolve to declarations in the
language's library files (`lib.d.ts`), read like any other file. Primitive
and default values (`number`, a bare `return`) are language-declared
`trait` recipes, never fake declarations.

## Calls and signatures

```zlint
in ts_function, ts_arrow, ts_method, ts_function_type, ts_call_signature, ts_method_signature {
  fits callable
  signature parameter returns return_type ?? returned
}
in ts_parameter { unwraps type ?? expected ?? init }
in ts_call { calls callee with argument; unwraps result }
in ts_new { constructs callee with argument; unwraps result }
in ts_declarator { in init expects type }
```

- `signature <field> returns <value>`: a callable's parameters (the
  field's nodes in order; one that `fits rest` takes the remaining
  arguments) and result. `returned` is the union of the values of the
  `returned` effects aimed at the node's scope. Optional `receiver <path>`
  (between parameters and `returns`) names the declared `this` parameter:
  no item = unknown, a path reaching none = known to have none.
- `calls <callee> with <field>` / `constructs ...`: the callee unwraps to
  candidates; those that fit `callable` (or `constructible`) with a
  signature are overloads, tried in declaration order: the first whose
  arity fits and whose parameters `could` take the arguments. `result` is
  the chosen signature's result, unwrapped `through` the call.
- `expected` (path step): the node's contextual value: an `in <field>
  expects <value>` of its parent; an argument's matching parameter of the
  chosen signature; a parameter's counterpart in its function's expected
  callable. `in <field> expects each <value>` evaluates `<value>` on each
  matched child instead (its own file, frame, parent and position; the
  condition and field selection stay on the owner), e.g.
  `in ts_array { in element expects each from parent.expected at position }`.
- `receiver` (path step, e.g. `it.expected.receiver`): the declared
  receiver values of every callable header the value reaches, without
  projecting results. No declared receivers = absent (`??` falls through);
  unknown metadata or a mix of present and absent = unknown; never stays
  never.
- `self` (value primary): the raw current leaf, same file and frame, not
  expanded (`it` keeps normal expansion). `unwraps self & constraint`
  keeps the declaration's own traits and carriers alongside the
  constraint.
- `overloads infer last [when <expr>]` (on a callable layer): inference
  (conditional `infer`, generic capture) uses the last signature; `accepts`
  still searches any compatible one; unset keeps the compatible search; an
  undecidable condition is unknown.
- Demand-specific queries (`call_parameters`, `call_results`) project only
  the requested side; unrelated unresolved signature parts never erase
  known answers.
- Generic inference: a type parameter reached through a call with no
  explicit argument unwraps to `inferred`: what the arguments give the
  parameter positions mentioning it (engine-owned).
- Explicit arguments read through the reference win; a present unknown
  slot blocks defaults and constraints.
- Inferred slots fill missing explicit arguments; declaration-only headers
  bind an empty inferred vector.
- A frame with no through reference and evaluated arguments is a
  synthetic library instance; never search a parent reference.

## Unwrapping: effects

Labelled lines in `unwraps { }` are effects: what evaluating the node does.
They are past participles, so each reads alone (`unwraps thrown argument`).

Engine effects (reserved; the engine acts on them):

| effect | meaning | engine use |
|---|---|---|
| `returned nearest <scope class>` | control leaves that scope (`return`) | never, reachability |
| `exited nearest <scope class>` | control leaves that scope without a returned value (an iterator closed from outside at `yield`) | never, reachability; not a `returned` payload |
| `thrown <x>` | throws `x`, leaving the nearest `try` or function | never, `could` over thrown values |
| `broken nearest <scope class>` / `continued nearest <scope class>` | `break` / `continue` | reachability |
| `truthy <unwrap>` | when the node is truthy, this holds | narrowing |
| `assigned <x>` | `x` is overwritten without reading it (`x = 1`, `for (x of xs)`) | a write; ends narrowing of `x` |
| `assigned <x> with <value>` | same, storing `<value>` (a values recipe, e.g. `with right`) | a write; narrowing of `x` restarts from its declared value reduced to what `<value>` could be |
| `mutated <x>` | `x` is read, then changed (`x += 1`, `x++`) | a read and a write; ends narrowing of `x` |
| `read <x>` | the binding `x` resolves to is read here (evaluated when the owner is entered) | indexed per binding; `flow_value_used` |
| `awaited <x>`, `yielded <x>` | waits on / yields `x` | async and generator rules |

User effects: any other participle (`unwraps logged argument`). The engine
does not interpret them; it propagates them (out of blocks, from callee
bodies to calls) so rules can ask about them. A user effect named like an
engine effect is a config error.

A block whose every path has `returned`, `exited`, `thrown`, `broken` or `continued`
unwraps to never; code after it is unreachable.

Within each phase, ordinary effects and possible controls keep declaration
order; the certain terminating control runs last. A selected entry (a
`switch` case) enters and leaves around its selected body only; falling
through never re-runs later tests.

## Traits

Rules ask questions about what a value could be. A trait is such a
question; leaf nodes say how they fit:

```zlint
in ts_null_keyword, ts_undefined_keyword { fits nullable }
in ts_function_type, ts_function, ts_arrow { fits callable }
match ts_interface ... { fits thenable when from it at "then" could callable }
```

Rules ask with two operators:

- `x could T`: some value `x` unwraps to fits `T`
- `x always T`: every value `x` unwraps to fits `T`

Over `|`, `could` needs one side and `always` both; over `&`, either side
decides both. Rules never see unwrapping.

```zlint
in ts_expression_statement rule no-floating-promises {
  when expression could thenable
}
```

Assignability is not an engine primitive. Rules ask traits on both sides
(the expected parameter `always void_returning`, the argument `could
thenable`). A rule that truly needs structural assignability gets a
dedicated engine primitive then.

## Narrowing

Three declarations and one engine rule:

1. **Where a test applies:** `in <field> propagates <test>` applies the
   test's `truthy` effects throughout that field's subtree: a static
   statement of where a test's exclusions go in the node's shape.
   ```zlint
   in ts_if { in consequent propagates test; in alternate propagates !test }
   in ts_logical {
     when operator == "&&" { in right propagates left }
     when operator == "||" { in right propagates !left }
   }
   ```
   `propagates` is generic: any value, trait or effect can be made known to a
   subtree (strict mode, an async body, the enclosing loop), not only tests.
   Derived by the engine: after an `if` whose consequent unwraps to never,
   the rest of the enclosing block gets `!test`.
2. **How tests compose:** a test's `truthy` effect unwraps into what must
   hold (`truthy left & right`, `!argument`). The engine pushes `!` inward.
3. **What a single test proves:** under `truthy`, `excludes`:
   ```zlint
   in ts_binary {
     when operator == "!=" && right is ts_null { unwraps truthy excludes nullable from left }
   }
   in ts_identifier { unwraps truthy excludes nullable, falsy }
   ```
4. **Engine:** a reference in a guarded region is narrowed only when it
   resolves to the same declaration as the tested operand and no `mutated`
   effect on it lies between test and use (code paths).

## Evaluation

- Everything is a pure function of the tree, links and other files'
  declarations: lazily computed, memoised per (node, question), cached in
  `.zlintcache` with recorded reads.
- Cycles and runaway unwrapping end in `unknown` under one engine-owned
  work budget per question; `could`/`always` on `unknown` are false. Rules
  and languages never write budgets.
- Declarations compile at config load into per-kind tables; the resolver,
  unwrapper and trait checks are engine Rust reading those tables. Purity
  also keeps ahead-of-time compilation possible later.

## World: files behind questions

One engine owner, `engine/world.rs`; type-layer engines see only the
`World` trait (`grammar/names/question.rs`).

- File 0 is the linted file (its tree, names and VM context come from the
  lint). Other files are **units**: path, text, tree, `Resolved` names.
  Loaded on demand when module resolution reaches them, never eagerly.
- Units are shared by every lint of a run (project cache keyed by path,
  dropped by `Linter::invalidate` for changed paths). Each lint that uses
  a unit records a read of its text (`engine/reads.rs`), so `.zlintcache`
  replays only while the text holds. Directory and package lookups go
  through registered `Discovery`s, never raw filesystem calls.
- A unit's tree holds only the layers name tables and their conditions
  need (computed at config build); its names resolve once at load, with
  conditions on the VM in a context over that unit. Native facts for a
  unit are built per lint when a condition or operand reads them.
- Per lint, a file table numbers the units a question reached (`u32`,
  0 = self); `NodeRef`, `ScopeRef` index into it. Questions never hold
  units past the lint.
- Module resolution: policy is zlint data (`module_resolution`, Daisy's
  resolver); the engine implements the resolver's `Host` (path, text,
  is_file, root scope, ambient module) over units and reads. Program
  settings (config, path mappings, base, libraries, types, project roots,
  program inputs) are anchored at the lint's root program and computed once
  per program, never re-derived from a visited foreign file; relative
  imports and packages stay anchored at the importing file. Templates may
  transform a placeholder (`{name:lower}`); an unknown transform is an error.
- One `Semantic` per root question, `Budget::DEFAULT`; nested engines
  reuse it. `condition` / `operand` / `attribute` run on the VM in the
  target node's file.
- Mechanics: `engine/world.rs` `LintWorld` per lint holds file 0's views
  (tree, text, names, native, attribute memo) plus an append-only arena of
  loaded units (shared `Arc` data: text, tree, names; per-lint memo + `Ctx`
  built on first use). `Ctx` carries `world`; the `Semantic` in progress
  lives in the world's active-question slot (set by a guard around each
  `condition` / `operand` / `attribute` run, restored on drop), so a
  condition that asks a type question nests in the same question. `Ctx`
  cannot hold it directly: `Ctx<'a>` is invariant (its memo holds
  `Value<'a>`), and questions are shorter-lived than the lint.
- Rules reach engines through a query registry (`expr/queries.rs`, like
  `FUNCTIONS` but given the host): `flow_normal(entry)`,
  `flow_declared_effects(entry, effect)`, later `could` / `always` /
  `member` / `accepts`. A query outside any question opens a root
  `Semantic`. First cut: answers holding nodes of other files or non-root
  frames are `null`; an opaque VM leaf handle comes after.
- A `Value::Node` names a node of the file whose `Ctx` evaluates it; it
  never carries a file. Rust receiving values from `condition` / `operand`
  / `attribute` reads them relative to the file it asked, and never feeds
  them into another file's `Ctx`. Cross-file nodes reach rules only as the
  opaque leaf handle (file + frame).

## Build order

Each step is accepted only when its facts are byte-identical to the native
implementation it replaces on wayward and the conformance corpus, and its
speed is within noise of native. Slower is a problem to solve, not a verdict.

1. **Names:** `scope`, `declares`, `resolves` for TS; replaces `bind.rs`.
2. **Values:** `unwraps` (values, generics via `through`) and `fits`,
   `could`/`always`; first consumers: no-floating-promises, await-thenable,
   no-misused-promises.
3. **Effects:** `returned`/`thrown`/`broken`/`continued`, never and
   reachability; consumers: no-unreachable, only-throw-error.
4. **Narrowing:** `propagates`, `truthy`, `excludes`, `mutated`; consumers:
   no-unnecessary-condition, strict-boolean-expressions.

## Step 1 prototype (in progress)

- Parser: `scope`, `declares`, `resolves` items in layer contexts
  (`config/ast.rs` `NameDef`); not reserved words (only item position).
- Build: per-matcher tables of scope classes, declarations, resolutions;
  config errors for classes resolved but never declared, `in nearest`
  classes nothing opens.
- Engine `engine/names.rs`: per tree, document order, scope stack by
  containment; `nearest` searches outward from the declaring node,
  including scopes the node itself opens; non-hoisted declarations are
  visible from their start; same name + scope merges into the first;
  references resolve after the walk.
- Rules read builtins `scope_declaration` (node or null) and
  `scope_references` (list); computed once per tree in the attribute memo.
  At cutover they become the `declaration` / `references` links.
- Check: a config rule reporting `ts_identifier` nodes where
  `scope_declaration` differs from the native `declaration`, run on
  wayward; zero reports and equal speed is the bar.
- Done: `targets` (patterns pass a declaration to their fields;
  per group the first alternative that declares anything), merging of
  hoisted declarations across classes, first lookup of a name wins (a
  parent's `resolves type name` before the name's own `resolves value`).
- `when cond { items }` (conditions compiled with the layer's fields,
  lowered to the VM; one session per tree runs them all) and
  `in enclosing <class>`.
- Wayward check (`scratch/names-proto/`): 0 of 661,376 native references
  resolve differently; 37 identifiers resolve where native says none or
  self. Lint wall +7% vs native on a names-only config, mostly the
  per-identifier `when` excluding name-only fields.
- Effects on targets: `unwraps <effect> <field>` (effects only so far)
  records the effect and its site on what the field reaches through
  `targets`, intermediate nodes included (`[a, b] = c` marks the array,
  `a` and `b`); builtins `scope_effect`, `scope_effect_site`. Counts
  (`uses`, `writes`) are derived by rules from `scope_references` and
  `scope_effect`, not stored.
- Next: check access and write sites against native on wayward; then
  `bind.rs` consumers move over and it goes. Also `propagates` for
  type-position context, implicit names (`arguments`), escaped names.

## Step 1 cutover (bind.rs goes)

- `std/ts/names.zlint` (from the prototype) imported by `.ts`: scopes,
  declarations, resolutions, targets, write effects for TS. Approximations
  in the prototype become exact (named function/class expression scopes,
  ES5 block functions, catch scope, root function scope).
- Engine owns one `Resolved` per tree (attribute memo). Native files get it
  from the engine before their first fact read; name conditions may read
  fields, kinds, text, options and name-free facts only.
- `Resolved` adds: scope table (opening node, classes, parent); declaring
  item node per declared name; classes asked per reference; references
  found nowhere; `visible(at, name, classes)`.
- Name keys: the front end normalizes identifier text (TS decodes escapes).
- Implicit names: `declares <class> "text" in nearest <scope>` declares a
  name whose declaration is the item's node (`arguments`).
- `bind.rs` becomes `Bindings::from_names(syntax, resolved)`: same fields
  for the ~37 consumers. Declaration kind, exported, ambient and self uses
  come from the declaring node's syntax; counts from references, asked
  classes and effects; this/yield/arguments flags move to a syntax pass.
- Accept: fixtures, full corpus vs the accepted baseline, wayward native
  facts identical where compared; speed within noise.

## Open

- Negative proofs (`x === undefined` proving nullable in the consequent):
  `excludes` covers the else side by negation; whether a positive form
  (`proves`) is also needed.
- The exact `fits ... when` expression vocabulary for member checks.
- How `through` behaves through aliases of aliases (`type A<T> = B<T>`).
- How rules and descendants read propagated context other than tests.
