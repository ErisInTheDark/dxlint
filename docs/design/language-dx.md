# Language DX decisions

Decided with the user; implementation queued in the engine. Each item lands as
config warnings first, std migrates, then the warning becomes an error.

## `unknown` (decided)

Give-ups are a language concept, separate from `null` (absent or empty).

- Producers: semantic queries, `could`/`always`, and the `unknown` literal (helpers that give up).
- Operators propagate it Kleene-style: `!unknown`, `unknown == x` stay unknown; `false && unknown` is false, `true || unknown` is true.
- `x is unknown`, `x not unknown`; `x is true` / `x is false` are definite answers (unknown → false).
- `x unknown y`: `y` when `x` is unknown, else `x`; same precedence tier as `??`. `??` never catches unknown.
- Parsing: after a complete operand `unknown` is the infix operator; in operand position it is the literal.
- Sinks (`when`, `report at`, `fix`, message interpolation, arithmetic): the compiler tracks a maybe-unknown bit; an unhandled maybe-unknown value at a sink is a config error. At runtime a sink that still receives unknown skips and is counted in `--stats` per rule.
- Query records keep nested uncertainty as explicit fields (`identity_known`, `uncertain_traits`, `carrier_known`); only top-level results become unknown.

## Macros only (rule-local `@macro` implemented; typed parameters, `fn` removal pending)

Logic is macros; there is no `fn` keyword. To the author a macro behaves exactly like token pasting: its body means what it would mean pasted at the call site. The engine is free to compile it however performs best (once and shared when the body reads only its parameters, specialised per site otherwise).

- `@macro` defines, `@name(...)` uses, for expressions as well as items and grammar.
- Parameters are typed (`node`, `text`, `number`, `bool`, `list`, `record`, `handle`); passing the wrong kind is a config error.
- No recursion (pasting cannot recurse): a cycle is a config error. Flat forms: `walk` (trees, chains, lockstep pairs as records), `fold`, and a new engine `fold_up(root, n => children, (n, results) => value)` for bottom-up values (iterative, memoised on DAGs, cycles give unknown).
- Macros may be declared inside an item or rule block; they are visible only there (`@present = ...` then `@present`).
- Migration: today's `fn` definitions and calls become typed expression macros; the 75 recursive ones are rewritten flat first.

## Nullability (`?.` and `{x?}` implemented as syntax; maybe-null tracking pending)

- Types are non-null by default; `T?` means `T | null` (on the type, never on the name), for macro parameters and option schemas alike.
- The compiler tracks maybe-null alongside maybe-unknown (optional fields, record keys, `find`, `first`, ...). Sinks (messages, `report at`, `fix`) need non-null values: handle absence with `??` or a guard first.
- `when x` and `x ?? y` keep working on nullable values.
- In template strings `{value?}` is sugar for `{value ?? ""}`: the explicit way to let absence print as nothing.
- Paths: `a?.b` reads through a maybe-null `a` (null stays null); plain `a.b` on a maybe-null `a` is a config error. Today every `.` silently passes null through; the migration rewrites those reads to `?.` where the compiler can't prove presence.

## `when` blocks (value, kind-subject and name/type item arm forms implemented; boolean subject and rule-body forms pending)

````dxlint
when {
  x then ...          # first true arm wins, no fall-through
  y then { ... }
  else then ...       # optional
}
when n is {           # subject form for kind dispatch
  ts_identifier then ...
  (ts_member, ts_index) then ...
  else then ...
}
````

- Works for items/rule bodies (arms hold items) and as a value (a missing `else` gives null, so the result is `T?`).
- Arm conditions are sinks: a maybe-unknown condition is a config error until handled (`is true`, `is false`, `x unknown y`).
- Replaces paired `when c {...} when !c {...}` blocks and long `if ... else if` ladders; `if a then b else c` stays for one-liners.

## Pipelines only (`|>` implemented; lambda call forms still accepted)

- Existing block pipelines stay as they are (`{ value xs; filter (x) ...; map (x) ... }`).
- `|>` writes the same pipeline inline and is equivalent to a newline: `sites |> any (site) site is ts_await`. `(site)` is the step's item parameter; `label:` after a newline or `|>` names the value that step produces (`xs |> kept: filter (x) ... |> count`).
- `value` only starts a new pipeline (the previous one ended); `value name: expr` is a new pipeline whose first result is labelled.
- No lambdas: call forms taking anonymous functions (`any(xs, x => ...)`, `filter(...)`, `map(...)`, `fold(xs, init, (s, x) => ...)`, `find`, `walk`'s next expression) become pipeline steps.

## `each` blocks (implemented; old block-less form still accepted)

````dxlint
in ts_assignment rule no-self-assign {
  when operator is ("=", "&&=", "||=", "??=")     # per node
  each self in problem_self_assignments(left, right) {
    let name re_replace("{self}", /\s+/, "")      # per item
    report at self
    warn "'{name}' is assigned to itself."
  }
}
````

- Lines inside the block run once per item and see it; lines outside run once per node and never see it.
- One form for option lists and code lists (`@each` goes); the engine knows which lists are fixed at config load.
- Prefer a rule on the item's own node kind when one exists; `each` is for derived items (pairs, groups) with no single owning node.

## Type recipes (`\`, `.[k]` and `trait a implies b` implemented; absorbing `any` pending)

- Trait implications are declared once: `trait undefined implies nullable, falsy, nonthenable`; `trait undefined` then means the whole set everywhere.
- Operators, tightest first: `a.[k]` member lookup (was `from a at k`), `!a` complement, `a \ b` difference (was `excludes b from a`), `a & b`, `a | b`, `a ?? b` (b when a is unknown). No comparisons, arithmetic or conditional types in recipes: yes/no questions stay in conditions (`could`, `always`, `fits`), branching stays in `when`.
- `any` absorbs: declared once (`trait any absorbs`); an operation on an absorbing value is that value, so recipes need no `any` passthrough branches.
- Boolean subject form: `when x is { true then ...; false then ...; unknown then ... }`. In recipes `unknown then unwraps unknown` keeps "can't tell" honest where an `else` would guess.

## Fields are nodes that read as text (decided)

- A field is its node; wherever text is expected (interpolation, joins, text functions, `~`, comparison with a string) the node reads as its raw source text.
- `node == node` is identity. There is no `.text`: a template string forces raw spelling (`"{a}" == "{b}"`), rarely right (prefer `identifier_name`, `reference_key`, `token_texts`).
- Repeated fields are always lists; lists of nodes join as their texts in messages.
- `.node` and `.nodes` go away.
