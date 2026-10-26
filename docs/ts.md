# TypeScript and JavaScript

`import .ts` loads dxlint's JavaScript and TypeScript library, including JSX/TSX syntax and rules. dxlint parses JS/TS itself and reads declared types using tsconfig, imported files, `@types` and `node_modules/typescript/lib`, without running TypeScript. Where a type cannot be told, rules stay quiet.

See [everyday use](basic.md) to run it, or the [advanced guide](advanced.md) to write rules.

## Contents

- [Overview](#overview)
- [Syntax layers](#syntax-layers)
- [Names and scopes](#names-and-scopes)
- [Modules and projects](#modules-and-projects)
- [Types and narrowing](#types-and-narrowing)
- [Language settings](#language-settings)
- [Facts](#facts)
- [Modules](#modules)

## Overview

`import .ts` (or `native ts` with your own JS language) parses
`.ts`/`.tsx`/`.js`/`.mjs`/`.cjs` files with dxlint's own TypeScript parser and
turns its syntax tree into `ts_*` layers: `ts_if` with fields `test`,
`consequent`, `alternate`; `ts_call` with `callee`, `type_argument`,
`argument`; `ts_binary`/`ts_logical` with `left`, `operator`, `right`, and so
on (`dxlint tree file.ts` shows any file). Rules on them work like any rule:

```dx
import .ts
in ts_call rule my-no-eval warn "no eval" { when callee == "eval" }
in ts_if.test, ts_while.test rule assign-in-condition warn "assignment in a condition" {
  when is ts_assignment
}
```

A file the parser cannot read gets one warning instead of rules on `ts_*`
layers.

## Syntax layers

**One tree.** A file the parser reads gets one tree: the parser's nodes, and
the pattern layers inside them, by the rules pattern trees follow: a pattern
layer inside a native layer fills the gaps between that node's other
children and sits under it, a named layer's matcher searches its whole text; the language's own
pattern layers (`in .js match ...`) are matched across the file and sit
under the program. Rules see no difference:

```dx
in ts_comment match todo /\bTODO\b/
in ts_string, ts_template match escape /\\./
in todo rule no-todo warn "Unexpected TODO."
```

**Tokens.** Punctuation and keywords that no child node covers are layers
too, under the node that owns them (field `token`): `ts_comma`,
`ts_semicolon`, `ts_colon`, `ts_dot`, `ts_question`, `ts_ellipsis`,
`ts_equals`, the brackets `ts_open_paren` ... `ts_close_angle` (groups
`opener` and `closer`), `ts_keyword` and `ts_punctuator`; group `ts_token`
has them all. `opener.x` and `closer.x` read a node's own first opening and
last closing bracket; `opener.ts_open_brace.x` only one of that layer (a
class's first opener is the `<` of `class A<T>`). Every native node knows `space_before` and
`space_after` (the gap to the neighbouring token) and the layers of
`previous_token` and `next_token`; `fix space_after as " "` rewrites just
that whitespace. Gap `blank_lines` counts physical blank lines; `blank_gaps` counts blank lines between tokens/comments, excluding comment interiors. Spacing rules are one decision per token:

```dx
in ts_comma rule es-comma-spacing:after warn "A space is required after ','." {
  when next_token not (closer, ts_comma) && !space_after.line_break && !space_after.spaced
  fix space_after as " "
}
in ts_block rule es-padded-blocks:top warn "Block must not be padded by blank lines." {
  when opener.space_after.blank_lines > 0
}
```

Files get token nodes only when an active rule needs them. A family name
turns on and configures every variant (`es-comma-spacing:before`,
`es-comma-spacing:after`): `rule es-comma-spacing`.

Comments and lines are pattern layers in `std/ts.dx` (see One tree):
`ts_line_comment` and `ts_block_comment` (group `ts_comment`) are children of
the code node whose text, between its other children, holds them, so never
inside a string, regex or template text; `ts_line` is in the named layer `lines`, over the
file, each JavaScript line with its line break (the last may have none).

Group `ts_node` holds every syntax layer (not tokens): `in ts_node let ...`
gives an attribute to all of them, for walks that may cross any kind.
`ts_code` is the same without literal text (`ts_string`, `ts_regex`,
`ts_jsx_text`).

**JSX tags.** `ts_jsx_element` and `ts_jsx_fragment` own `opening`, `body`
and `closing`. Their `ts_jsx_opening` and `ts_jsx_closing` nodes cover the
whole tags, including punctuation. Opening tags own `name`,
`type_argument`, `property` (attributes) and `token`; closing tags own
`name` and `token`. Fragment tags have no name or attributes; self-closing
elements have no closing tag. Children stay in the element or fragment's
`body`, separate from both tag owners.

Project rules can style opening-tag lines without changing child indentation:

```dx
import .ts
rule es-indent {
  set mode 2
  indent in ts_jsx_opening by 3
}
```

```tsx
<Box
      prop="value"
>
  Text
</Box>
```

`override in ts_jsx_opening let indent_reset true` instead keeps opening-tag
lines at their physical indentation. Child text, closing tags and nested
attribute expressions remain checked. This selects every opening tag,
including nested elements; use a predicate such as `name == "Box"` to keep
other opening tags checked. Read tag bounds from `opening.offset`,
`opening.end_offset`, `closing.offset` and `closing.end_offset`, and tag
fields from their tag owner rather than the enclosing element.

## Names and scopes

**Name attributes** (`.ts/names`, null elsewhere):

| name | value |
|---|---|
| `binding_declarations`, `declaration`, `declarations`, `references` | node-valued binding links (usable as rule targets, `in K.attr rule`) |
| `declaration_kind` | on a binding's canonical first declaring identifier: its syntax kind (`const`, `let`, `var`, `parameter`, `type_parameter`, `function`, `function_name`, `class`, `class_name`, `import`, `type`, `interface`, `enum`, `namespace`, `catch`, `using`) |
| `declaration_kinds` | on any declaring identifier: the syntax kinds of every declaration merged into its binding, in source order; null on references |
| `reference_kind` | on a referencing identifier: the syntax kind of its binding's canonical first declaration; null when unresolved |
| `uses`, `value_uses` | on a declaring identifier: how often its name is used, and how many of those uses need the runtime value (type positions, and `typeof x` or `[x]` keys inside types, do not) |
| `self_uses`, `writes` | uses inside its own function, and assignments to it |
| `ambient` | on a canonical declaring identifier: every merged declaration is inside `declare` |
| `resolved` | on a referencing identifier: `local`, `import` or `global` (declared nowhere in the file) |

Values and types are separate namespaces (`const X` does not hide an
imported type `X`), and merged declarations (`interface X` + `const X`,
overloads) are one binding.

## Modules and projects

Types are read from the files
they are declared in (other project files, `node_modules` packages and
`@types`, `declare module "x"`), with the nearest `tsconfig.json`'s `paths`,
`lib`, `types` and `references` deciding what is visible. Other files are
parsed once per run, when a question first needs them, and read like the
linted file: `export const v = w` in another file is typed from `w`.

The `.ts/modules` library declares publications, import links, namespaces, globals and module-resolution profiles. The [facts reference](#facts) includes queries for module exports, program options and declaration origins.

## Types and narrowing

**Types** come from declarations: annotations, return types, initializers
and member chains, generic arguments fixed by an argument (`map(x => ...)`),
`await`, `as`, `!`, object literals, `keyof`.

- **Overloads** are chosen by argument types (a bounded assignability check:
  primitives, literals, enums, unions, arrays, tuples, the same generic, and
  small object types property by property). When the arguments cannot pick
  between overloads that return different types, the call is unknown.
- **Narrowing** is local and lazy: a reference's type is refined by the
  conditions guarding it in the same function (`if`/`else`, `?:`, `&&`,
  `||`, early `return`/`throw`) when they test it with `typeof`,
  `=== null`/`undefined`, truthiness, `instanceof` or `Array.isArray`. An
  assignment to it in between cancels the guards; anything else leaves the
  declared type.

Where a type cannot be told, `type` is null and rules stay quiet; there is
no contextual inference beyond callback parameters.

### Instantiation, mapped types and limits

`callable_contracts(parent, node, ["[Symbol.dispose]"])` asks only about that member, without traversing unrelated fields. `[Symbol.<name>]` path selectors require real Symbol identity on both sides, never printed-string properties. Symbol keys retain canonical library identity through proven constructor aliases and unique-symbol member types; arbitrary computed keys remain unknown. Ordinary object-property contexts still match string keys to string keys.

Explicit instantiation (`fn<T>` or `typeof fn<T>`) binds each declared function signature independently; later calls do not infer those parameters again. Constructor instantiation, callable-object instantiation and functions with inferred body-return overrides remain unsupported and produce unknown types.

Mapped property types retain their key domain, dependent value type, optionality and readonly flag. Indexed-key transfers preserve computed-key declaration identity and prefer ordinary declared properties. Key remapping (`[P in K as ...]`) remains unsupported; key domains that cannot be enumerated do not imply an empty property bag.

## Language settings

Language settings describe the source environment, not a rule's policy. `import .ts` declares the JavaScript/TypeScript environment on `.js`:

```dx
import .ts
language .js {
  files "scripts/**"
  ignore "scripts/generated/**"
  set globals { app: "readonly", cache: "writable", removed: "off" }
  set ecmaVersion 5
  set sourceType "commonjs"
  set impliedStrict false
  set globalReturn true
}
```

- `files` and `ignore` select `set` clauses using config-relative globs, as rule filters do. Without `files`, settings apply to every file of that language. Option schemas/defaults belong to the language, independent of filters. Later matching blocks override scalar settings; global maps merge by name, including explicit `"off"`. Real source global comments override configured names without becoming configuration.
- `ecmaVersion` accepts `3`, `5`, editions `6` through `17`, years `2015` through `2026`, `"latest"` or `null`. Unset/`null` means latest. The edition selects built-in globals and directive semantics; it does not reject every newer syntax feature.
- `sourceType` accepts `"script"`, `"module"`, `"commonjs"` or `null`. Unset/`null` preserves filename/import inference. Modules are strict; CommonJS adds a root function scope and CommonJS globals.
- `impliedStrict` and `globalReturn` accept booleans or `null` (false). `impliedStrict` supplies strictness without fabricating a directive. `globalReturn` adds a script root function scope, not CommonJS globals; modules do not acquire that wrapper.
- A language declares supported settings with `language .name { option name <type> = <default> }`. Unknown settings and values outside the declared type are errors. Changing language settings invalidates cached lint results.

## Facts

Native layers add builtins (null elsewhere): properties of the
code any rule may ask about. What a rule makes of them (its options, what
is wrong, messages) is written in the rule. A fact taking arguments is called (`type_structure(["then"])`); on another node, call it there: `source.node.type_structure(names)` asks about `source`, with `names` read where the call is written. A name that is also a field label reads the field (null on kinds without it); its fact is read only as a call (`type()`).

| name | value |
|---|---|
| `symbol_metadata` | `{name, own_doc, declarations, aliases, selected_signature}`; raw declaration and alias docs, overload `signature_docs`, declaration kinds and file/lib/package provenance gates. Alias rows retain lookup order, path and module; `selected_signature` is null or `{doc, kind}` (`call`/`construct`). Ambient package gates retain ordinary file provenance. Rust exposes docs and origins; `.dx` decides deprecation |
| `type` | the expression's type as text (`Promise<void>`, `never`, `NodeJS.Timeout`), null when unknown |
| `argument_transfers` | calls, constructions and tagged templates: any/generic argument and spread transfer rows `{offset, mode, error, sender, receiver}`; modes `argument`, `spread`, `array`, `tuple`; `error` marks unresolved provenance, sender/receiver are raw printed types or null |
| `return_transfer` | returns and expression arrow bodies: transfer record `{mode, error, sender, receiver}` or null; modes `value`, `array`, `promise`, `generic`; `error` marks unresolved provenance, sender/receiver are raw printed types or null |
| `index_key_types` | constrained index-signature key domains as `{text, string_like}` records, inherited domains included |
| `type_relations` | operand, annotation, original and context identity, bounded assignability, nested property mutability and generic shape; null when unresolved |
| `type_flow(site, source)` | source/expected type relation records at declared, contextual, argument, return, indexed-key and heritage sites: union domains and enum identities, tri-state assignability, aligned generic/index/property children, typed operands and receiver enum names; call receivers retain declared generic bounds or explicit type arguments, not inference from the checked value; `complete: false` marks traversal limits |
| `type_awaitability` | structural thenable union facts `{any, all, union_equivalent, complete}`; `all` requires every non-never arm to be known thenable, `any` needs one; `union_equivalent` is mutual assignability between awaited and ordinary arms of a known mixed union, or null |
| `callable_contracts(site, source, member_path?, fields?)` | callable transfer rows `{origin, source, target, heritage, member_path, complete}`; source/target contain return arms `{kind, thenable}`. Origins are `argument`, `return`, `declared`, `contextual`, `heritage` or `resource`; heritage is a direct base node or null. The optional string-list path selects exactly one member chain; omitted or `[]` selects the root. Argument contracts include every visible overload, not only the selected one; unresolved signatures or work limits mark rows incomplete. The optional fourth string-list argument adds `contextual_returns`: raw recursive `{kind, parts}` return profiles at the selected member path, retaining unions, intersections and open signature parameters without argument inference; `[]` means known non-callable, null means unavailable, opaque or work-limited. Omitted fields do no extra contextual lookup and keep the original row shape. Example: `callable_contracts(node, node, [], ["contextual_returns"])` |
| `syntax_tree(node, name, input)` | parses a declared grammar over `{ text, flags?, spans?, offset, end_offset }` in `node`'s file; returns `{ ok, root, nodes, error }`. `spans` maps each Unicode scalar to an absolute `{ offset, end_offset }` byte span; omitted/null maps every node to the whole extent. Nodes are plain id/parent/kind/text/raw/children/fields records with local `start`/`end` and mapped offsets; invalid parser/input/syntax returns `ok: false`, unavailable answers return null |
| `node_ordinal(node)` | zero-based position among the parent's children in the same field; null for the root or a non-node. Unlike `node.index`, never shadowed by a syntax field named `index` |
| `js_string_parts(text)` | decodes a quoted string or substitution-free template to `{ text, spans, units }`; spans map Unicode scalars and units keep UTF-16 values, both with byte offsets inside the literal. Null for invalid syntax; lone surrogate units stay distinct while scalar text uses U+FFFD |
| `re_valid(pattern)` | whether a pattern compiles in the same regex dialect and cache as `re_test`; false for invalid syntax (not JavaScript regexp validation) |
| `environment` | the file's raw lint environment settings in config order, `{settings: [{name, value}]}` (`ecmaVersion`, `sourceType`, `globals`, ...); languages fold and normalise them |
| `name_scopes(node, role)` | the scope chain for `node` in `role` (`declaration`, `reference` or `lexical`), innermost first, as `{id, owner, classes}` rows; `id` is an opaque scope identity |
| `name_scope_declarations(node, scope, key)` | the declarations of `key` in scope `scope` (an id from `name_scopes`), in source order |
| `name_definitions(node, class)` | the declarations the name `node` finally stands for as `class` (a declaration class name), followed through imports and re-exports, as `{file, node}` sites (`[]` when it names nothing of that class); null when unknown |
| `name_binding(node)` | the explicit binding `node` declares or uses, as `{node, key, scope, declarations, references}`: `node` is the canonical (first) declaration, `scope` a `name_scopes` row, lists in source order; null when unbound, not a binding, or implicit; unknown when incomplete or over budget |
| `name_role_classes(node, role)` | the declaration classes `node` takes in `role`. For the `name_*` queries, a valid node simply lacking the role answers null (known absent); an invalid role/scope id, failed names or an exhausted budget is unknown |
| `name_classes(node, key, source, classes)` | per declaration class, does `key` (looked up from `node`, or in module `source` when given) declare a name of that class: `true` / `false` / `null` when unknown, independently per class. `classes` null asks every class; a list asks (and returns) only those, so ask only what you need. Unknown class names give null |
| `module_exports(node, source)` | the complete publications of module `source` as `{name, classes}` rows (`classes`: per class `true`/`false`/`null`); null when the inventory is incomplete or cyclic |
| `program_option(node, name)` | option `name` of the configuration the program around `node` loads (its `module_resolution` config `options`, after inheritance), as JSON values; null when unset or unknown |
| `program_directory(node)` | the directory of that configuration file, `/`-separated (compare with `declaration_origin(...).path` for program-relative paths); null when the program loads none |
| `value_constituents(node)` | the node's value as nested union/intersection lists of `{traits, uncertain_traits, identity, identity_known, carrier, carrier_known, product, product_known}` records (type layers); `carrier` is `{file, node, description, arguments}` with arguments projected the same way, eagerly (an unknown argument makes the carrier unknown: use `value_parts` for lazy argument handles); `null` when unknown; `[]` is never |
| `value_constituent_traits(node, traits)`, `value_trait_parts(node, traits)` | `value_constituents` / `value_parts` reduced to the listed trait names: each constituent is only `{traits, uncertain_traits}` over those traits (plus `handle` in the parts form), skipping every other trait and the identity, carrier and product records. For callers that test a few traits; null when unknown or a name isn't a trait. Take a handle too |
| `value_at(node, keys)`, `value_expected_at(node, keys)` | the `value_constituents` shape after an ordered member lookup (text or non-negative integer keys; `[]` is the node itself), through the value or through the expected (contextual) value; null when invalid or unknown |
| `value_member(node, key)` | one member's value, in the `value_constituents` shape |
| `value_expected(node)` | the contextual value: null when unknown, `{present: false}` when known absent, `{present: true, value}` otherwise |
| `value_has_expected(node)` | true when contextual type exists, false when known absent, null when unknown; does not expand the value |
| `value_widened(node)` | the widened value (`widens` recipe), in the `value_constituents` shape |
| `value_accepts(node, other, allowing, given)` | whether every value `other` may be is one `node` allows (assignability), each side read as its `"value"`, `"expected"` or `"widened"` value; null when either side is unknown or absent |
| `node_site(node)` | the node as a `{file, node}` site record (canonical semantic node), comparable with the sites other queries return (`call_binding` arguments, sources); null for a non-node |
| `value_expected_handle(node)` | the value the node's context expects (as `value_expected`), as a handle; null when nothing is expected or unknown |
| `value_union(node, [handle, ...])` | one handle for the union of the node's value and each listed handle (unions compose: never is the identity); null when any input is unknown. Takes a handle too |
| `value_same(node, handle)` | true when the node's value (or a handle) is provably the same value as the handle's: equal, or instances of one declaration with the same arguments (`Map<string, string>` reached from two sites); a modified value (`Readonly<T>`) is never the same as its source. Null otherwise (never false). Takes a handle too |
| `value_alternatives(node)` | the node's value as one handle per union arm, without describing any (cheap where `value_parts` would project every trait); `[]` is never; null when unknown. Takes a handle too |
| `value_parts(node)` | the node's value as union arms `{handle, parts}`: `handle` stands for the whole arm (its intersection constituents together), `parts` are its constituents in the `value_constituents` shape, each with its own `handle`, except that carrier `arguments` are handles (left unread); null when unknown. A handle is an opaque `{value_handle}` record: pass it where these queries take `node` (`value_parts`, `value_children`, `value_constituents`, `value_at`, and with a text or ordinal key `value_lookup`, `value_has_member` and `member_call_parameters`; `value_accepts(allowing, given, _, _)` with two handles) or compare it for equality (same value, same handle, with frames that bind nothing folded so recursive types revisit the same handle). Handles are valid only while linting the file that made them; any other value reads as null |
| `value_children(node, members, arguments)` | the edges out of one arm (a node whose value has one arm, or an arm handle): with `members` true, every member of the arm as `{handle, member: {key, optional, sources}}` (sources as in `value_lookup`, plus their `flags`), only when the arm's members are all known (closed; constituents declaring no members of their own, such as trait-only parts (open ones too: check a part's `uncertain_traits`) and function types, add none and don't open it; tuple products list their elements (a rest element's handle is its items, marked `repeated`; a directly repeated element such as an array literal's item is marked `values` and its handle is the element itself), never inherited methods); with `arguments` a list of indexes, those carrier arguments of each constituent as `{handle, argument, part}` (`part` indexes the arm's constituents). `arguments` null (an unknown selection), a value with several arms or anything unknown gives null. Rules own recursion, pruning and cycles; recursive generics never repeat a handle (each level binds anew), so past a fixed instantiation depth the answer is null |
| `value_signatures(node)` | the call signatures of each union arm of a node's value or a handle, as `{parameters, receiver, has_receiver, result}` per header: `parameters` a list of handles (null when unreadable), `receiver` and `result` handles (null when absent or unreadable), `has_receiver` false when there is none, null when unknown; null when the callables are unknown. Unlike `call_signatures`, nothing is read until a handle is asked. Each header also has `site`, the declaring `{file, node}` (null when not a node) |
| `value_bases(node)` | for one constituent (a part handle, or a value with a single leaf): the values it inherits members from, as handles in declaration order (declared bases of its view, a tuple's array base, a primitive's apparent wrapper); `[]` when none, null when unknown or not one constituent. Walk them flatly to find ancestry (`walk([h], h => value_bases(h))`) |
| `value_constructors(node)` | `value_signatures` for `new`: the construct signatures |
| `declaration_origin(node, site)` | where a `{file, node}` site's file comes from, by path under the program's resolution profile: `{path, kind, package}` with `kind` `"library"` (fits the profile's `libraries` template), `"package"` (inside a package directory; `package` names it, scope included) or `"project"`; null when unknown. Builtin identity = a global declaration site whose origin is `library` |
| `declaration_value(node, site)` | the value a `{file, node}` declaration site unwraps to (any program file, root frame) as a value handle; null when unknown or not a site. E.g. a library global's value side, to compare constructor identities |
| `value_key(node)` | the member key a key expression evaluates to (`obj[key]`'s `key`): text, an ordinal, `{symbol: {file, node}}` or a wide `{domain}`; null when unknown. Pass it to `value_lookup` (node or handle form) |
| `value_keys(node)` | every key a key expression may evaluate to, in `value_key`'s forms: a finite union (`'a' \| 'b'`) gives each literal key, where `value_key` widens them to `{domain}`; null when unknown. Look each up to keep named members of a finite key set |
| `call_binding(node)` | on a call or `new`: its selected signatures in their bound frames (generic parameters instantiated for this call): `{uncertain, choices: [{site, result, parameters: [{site, rest, value, arguments}]}]}`, `result` and `value` as value handles, `arguments` the `{file, node}` sites each parameter receives (several for a rest parameter); null when not an invocation. `call_binding_at(node, ["return_type", "type"])` adds `at` to each choice: the value at that field path from the signature's node, in the bound frame (a type guard's instantiated target) |
| `value_sources(node)`, `site_sources(node, site)` | one step of where a value comes from, read off the language's value recipe (the site form reads any program file at the root frame): `{combine, sites}`; a reference's site is the declaration it resolves to, union and intersection recipes list their parts (`combine` `union` / `intersection`, else `one`). Sites are `{file, node, kind, name}` (`name`: the declared name, if any). `sites` is empty for a node holding its own value; null when the recipe can't be followed (computed values, products, calls). E.g. walk an annotation's alias route per union arm |
| `site_field(node, site, field)` | the children of a `{file, node}` site under the named field, in any program file, as `{file, node, kind, name}` sites (as `value_sources`); `[]` when the field is empty, null when not a site or an unknown field. Read a foreign declaration's parts (a class's type parameters, then `declaration_value` of each default) |
| `site_parent(node, site)` | the parent of a `{file, node}` site in any program file, as a `{file, node, kind, name}` site; null at the root or when not a site. E.g. walk from a declaration to the export wrapper that carries its docs |
| `site_text(node, site)` | the source of a `{file, node}` site in any program file, as `{text, leading}` (`leading`: the text between the previous sibling, or the parent's start, and the site — its comments); null when unavailable. `node` is any node of the linted file |
| `call_selection(node)` | on a call or `new`: the headers it selects, `{selected: [{file, node}], uncertain}` (several when overloads stay open); null when not an invocation or unknown |
| `value_origin(node)` | the nearest instantiating reference `{file, node, kind}`; null when absent or unknown |
| `global_declarations(node, name, class)` | the global declarations of `name` in `class` as `{file, node}` rows; `[]` known absent, null unknown |
| `declaration_identity(node)` | the canonical declaration `{file, node}` a name or value comes from; null when unknown |
| `member_call_parameters(node, key)` | the resolved member `key`'s call parameters only (like `call_parameters`), without results or receivers; null when unknown |
| `call_parameters(node)` | the call signatures' parameters only (`{parameters, parameters_known}` per overload), without evaluating results or receivers |
| `call_results(node)` | the call signatures' results only (`{result, result_known}` per header, `result` as `value_constituents` rows or null), without parameters or receivers; null when the callables are unknown |
| `value_lookup(node, key)` | the member `key` of `node`'s value: `{member, alternatives: [{matched, required, sources: [{kind, domain, site, flags}]}]}`; `kind` is `named`, `ordinal`, `repeated` or `indexer` (`domain` on indexers only); `site` is the declaring `{file, node}` (null when not a node); `flags` are the declaration's `flag` names. `key` is text, an ordinal, a key record (`value_key`'s `{symbol: {file, node}}` / `{domain: "text" | "ordinal" | "symbol"}`, also accepted by the handle form), a node (its evaluated key) or a `{global, class, path}` recipe (`{global: "Symbol", class: "value", path: ["asyncIterator"]}`). Named members win over index signatures; looks up the value as given (no optional-chain stripping). `member` is the member's value as a handle (frames kept: chain further lookups, `value_parts`, `value_constituents`, `value_accepts` from it); null when unknown. The member's value is not described here: describe it from `member` only when needed |
| `value_member_optional(node, key)` | whether some alternative of `node`'s value declares member `key` as a named member that is not required (`value_lookup`'s `matched && required == false` with a `named` source, without building the member's value); `key` as for `value_lookup`; null when unknown |
| `value_has_member(node, key, source)` | true when any alternative has a named member `key` (optional members count: declared, not guaranteed at runtime); false only when every alternative is closed and lacks it (numeric and symbol indexers don't open a type for named keys); null when unknown, open, indexer-only or the key is unresolved. `source` null asks `node`'s value; `{ global: "Array", class: "type", path: [] }` asks the real global declaration's value, following `path` members |
| `call_signatures(node)` | the node's call signatures as records with parameter, result and receiver constituents and their knownness (type layers), plus `parameter_rest` / `parameter_optional` (one bool or null per parameter, in order); `null` when unknown |
| `expected_call_signatures(node)` | `call_signatures` of the node's expected value; alternatives that cannot be called drop out; `null` when nothing is expected or unknown |
| `flow_entry(node)` | the execution entry (body) holding `node` |
| `flow_normal(entry)` | can `entry` complete normally: `true` / `false`, `null` when unknown |
| `flow_declared_effects(entry, effect)` | the `effect` events declared in `entry` (records with `site`, `target`, `value`) |
| `flow_exits(entry, effect)` | the reachable `effect` completions leaving `entry` (records with `site`, `target`, `value`), unlike declared events |
| `flow_prior(node, effect)` | does some path from the entry reach `node` after an `effect` |
| `flow_required(node, effect)` | does every reachable path to `node` pass an `effect` (unreachable: `false`) |
| `flow_complete_without(entry, effect, exits)`, `flow_complete_with(...)` | can `entry` complete normally or through one of the `exits` effects on a path avoiding / passing `effect` |
| `flow_crosses(entry, starts, observations, effects, barriers)` | is there a reachable route start → one of the `effects` → an observation, with no barrier entered after the effect (barriers before it don't cut); null when unknown |
| `flow_has_reads(binding)` | does `binding` have any declared reads anywhere in the file (indexed) |
| `flow_value_used(target)` | is a write to `target` consumed by a declared read (`unwraps read`) before the same binding is overwritten; reads from another entry count as used |
| `type_structure(names, fields?, constrained?, member_path?)` | constrained recursive `{kind, parts, names, generic_names, aliases, members}`: array/tuple elements or union/intersection parts; optional tuple slots include undefined. Explicit `unknown`, open `parameter` and `unresolved` are distinct. `names` includes printed base types; `generic_names` contains root generic declaration names. `aliases` contains retained root-annotation alias names, not aliases propagated through inferred values or calls. Queried members contain winning `{owner, kind, signature, key_symbol}` declarations; owner is `{name, kind, from}` or null, and `key_symbol` verifies standard-library Symbol computed-key identity. The second argument selects the recursive fields returned (default: the six above); opt-in fields: `declarations` (`{name, kind, from, inherited}`), `callable`, `constructor` (declared construct signatures or class values), `symbol_constructor` (a named type's constructible value side), `class_value`, `property_names` (without signature markers or apparent function prototype members), `iterable` (null when unresolved), `arguments` (resolved generic type arguments, each with the selected fields; not callable parameters), `member_types` (selected names → `{name, type}` records; resolved inherited/generic members; nested descriptors keep selected fields), `call_signatures` (all visible declared call contracts → `{parameters: [type descriptors], rest}`; declared count, no call-argument inference; constrained mode resolves bounds, raw mode keeps open parameters; `[]` means known non-callable, null means opaque/work limit; parameter and receiver descriptors omit `members`, `member_types`, `call_signatures`, `member_metadata`, `call_signature_metadata`), `member_metadata` (selected names → winning declarations `{owner, kind, is_static, initializer_kind, provenance, signatures}`; `initializer_kind` is the raw function/arrow/other/null syntax), `call_signature_metadata` (visible declaration headers `{receiver}`; receiver null (no explicit `this` parameter) or `{name, annotation: {kind, keyword} | null, type}`; null metadata means opaque/work limit, `[]` known non-callable) and, on the root, `property_writes` (property names assigned through the binding). The fourth argument `member_path` follows the selected member names before reflection (finite; opaque on depth/work exhaustion). The third argument (default `true`) constrains; `false` keeps open type parameters and named wrappers (`Readonly<T>`) unpeeled: `type_structure([], ["kind", "parts", "declarations", "arguments"], false)`. Example selected member and callable structure: `type_structure(["then"], ["kind", "parts", "member_types", "call_signatures", "callable"])` |
| `type_parameter_defaults` | on a call, `new`, type reference or class with type arguments: each type parameter's default, printed |
| `type_argument_redundant` | on the last type argument: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types assignable both ways count as the same, as for typescript-eslint) |
| `regex_problems` | on a regex literal or the pattern string of `RegExp(...)`: one problem per line: `empty_class`, `spaces:N`, `backref:<kind>:<backref>:<group>`, `misleading:<kind>` |
| `regex_error` | on the pattern string of `RegExp(...)`: the syntax error (or bad flags) as ESLint words it |
| `identifier_name` | on an identifier or private name: the name as JavaScript reads it (`foo` is `foo`) |

## Modules

Importing `.ts` loads its topic files and shared building blocks automatically; their rules stay off until enabled. These modules are internal building blocks, not separate rule bundles.

A language's rules live in topic files it imports itself (`.ts/format`,
`.ts/layout`, `.ts/indentation`, `.ts/style` with its parts `.ts/style/ordering`,
`.ts/style/signatures`, ...; `.rs/format`): internal building blocks, not
separate bundles.

| language | contents |
|---|---|
| `.ts` | `native ts` for every JS and TS file: `ts_*` layers, comments (`ts_comment`) and lines (`ts_line`), the groups `ts_node`, `ts_code`, `ts_function_like`, `ts_loop`, `ts_statement`, and the eslint, @stylistic and typescript-eslint ports |
| `.ts/import-attributes` | shared pattern-owned import/export attributes for key spacing and trailing commas; loaded by the rule modules that need it |
| `.ts/conditions` | type-aware condition policy, imported by `.ts`; optional-chain suggestions remain diagnostic-only |
| `.ts/arithmetic` | type-aware addition policy and options, imported by `.ts/typed` |
| `.ts/expressions` | boolean-cast, exponentiation and logical-assignment policy with precedence-safe fixes, imported by `.ts/suggestions` |
| `.ts/expression-fixes` | shared precedence, token-boundary and ASI helpers for expression autofixes |
| `.ts/literal-text` | shared pattern-owned string/template text pieces for escapes and quote fixes |
| `.ts/layout/blocks` | block braces, spacing, padding and gaps between class members, imported by `.ts/layout` |
| `.ts/layout/calls` | declaration/call parentheses, optional call spacing and constructor parentheses, imported by `.ts/layout` |
| `.ts/layout/expressions` | operator, star, rest/spread and property-access spacing, imported by `.ts/layout` |
| `.ts/layout/separators` | comma and statement semicolon policies, imported by `.ts/layout` |
| `.ts/layout/types` | member separators, annotations, generics and named tuples, imported by `.ts/layout` |
| `.ts/layout/literals` | quotes, property quoting and floating decimal notation, imported by `.ts/layout` |
| `.ts/layout/text` | comment spacing, repeated whitespace and blank-line limits, imported by `.ts/layout` |
| `.ts/layout/shared` | shared value-only spacing helpers and comment pattern for the layout topics; no rules |
| `.ts/template-fixes` | shared raw-escape-preserving template fix helpers |
| `.ts/style/type-imports` | consistent type import policy, usage and safe autofixes |
| `.ts/style/type-exports` | consistent type export policy and safe autofixes |
| `.ts/suggestions/object-shorthand` | object member short/long syntax policy and source-preserving fixes |
| `.ts/suggestions/curly` | control-body brace policy and statement-safe fixes |
| `.ts/suggestions/renames` | redundant import, export and destructuring aliases; comment-safe fixes |
| `.ts/restrictions/comments` | shared comment metadata, inline and warning-term policies, and Unicode-safe capitalization fixes, imported by `.ts/restrictions` |
| `.ts/suggestions/object-spread` | Object.assign eligibility and source-preserving object spread fixes, imported by `.ts/suggestions` |
| `.ts/suggestions/templates` | string concatenation policies and source-preserving template fixes, imported by `.ts/suggestions` |
| `.ts/suggestions/computed-keys` | unnecessary computed literal keys and comment-preserving, token-safe fixes, imported by `.ts/suggestions` |
| `.ts/suggestions/comparisons` | literal-side comparison policy, range exceptions and source-preserving operand swaps |
| `.ts/typed/optional-chains` | optional-chain guard selection, return-type safety and atomic fixes, imported by `.ts/typed` |
| `.ts/typed/boolean-comparisons` | boolean literal comparisons, nullable options and source-preserving fixes, imported by `.ts/typed` |
| `.ts/typed/void-expressions` | void expressions in statement positions, imported by `.ts/typed` |
| `.ts/typed/type-constituents` | duplicate union and intersection constituents by type identity, with token-safe removal fixes, imported by `.ts/typed` |
| `.ts/typed/readonly-members` | readonly private members and parameter properties, with type-preserving annotation fixes, imported by `.ts/typed` |
| `.ts/typed/readonly-parameters` | deep readonly parameter types and options; `allow` needs alias/provenance metadata; mapped and conditional views pending; imported by `.ts/typed` |
| `.ts/typed/rejection-handlers` | inline rejection handler `unknown` policy, including branch, sequence and rest-element selection; imported by `.ts/typed` |
| `.ts/typed/unsafe-members` | no-unsafe-member-access dotted/computed access and key checks, using value-handle traits; imported by `.ts/typed` |
| `.ts/typed/unsafe-calls` | no-unsafe-call call, construction and template-tag checks, using value-handle traits and library Function signatures; imported by `.ts/typed` |
| `.ts/typed/type-conversions` | primitive conversion policy for calls and operators, including enum and integer proofs, imported by `.ts/typed` |
| `.ts/typed/redundant-constituents` | redundant union/intersection top, bottom and primitive/literal constituents, imported by `.ts/typed` |
| `.ts/typed/string-boundaries` | string prefix/suffix tests (`startsWith`/`endsWith`) and source-preserving fixes, imported by `.ts/typed` |
| `.ts/typed/return-await` | returned-await policy and source-preserving fixes, imported by `.ts/typed` |
| `.ts/typed/return-this` | class return annotations over declared returns, unwritten this aliases and generic class identity; imported by `.ts/typed` |
| `.ts/typed/nullish-coalescing` | stable nullish replacements, primitive exclusions and context selection, imported by `.ts/typed`; diagnostic-only |
| `.ts/typed/template-expressions` | unnecessary template interpolation checks and raw-escape-preserving, precedence-safe fixes, imported by `.ts/typed` |
| `.ts/typed/floating-promises` | floating promises, including discarded arrays and tuples with immediate promise elements, imported by `.ts/typed` |
| `.ts/comments` | JS/TS comment layers (`ts_line_comment`, `ts_block_comment`, group `ts_comment`) under the code layer; imported by `.ts` and by modules that read comments |
| `.ts/tokens` | shared token edges, pairs, code-token text and comment-aware gaps over the active front end; imported by `.ts` |
| `.ts/precedence` | operator precedence and grouping of expression operands; imported by `.ts/structure` |
| `.ts/structure/declarations` | declaration shapes shared by structure policies; imported by `.ts/structure` |
| `.ts/structure/lists` | list layout: delimiters, items and token edges of every list-like node; imported by `.ts/structure` |
| `.ts/structure/lines` | line facts: byte-order mark and operand line records; imported by `.ts/structure` |
| `.ts/typed/transfers` | getter/setter type agreement over value handles; imported by `.ts/typed` |
| `.ts/typed/defaults` | useless default values on optional properties and contextual parameters, using parameter rest/optional flags; imported by `.ts/typed` |
| `.ts/typed/template-values` | template interpolation type policy as union/intersection clauses with options and allowed named types; imported by `.ts/typed` |
| `.ts/typed/enum-consistency` | mixed string/number enum members across merged, namespaced and imported declarations; imported by `.ts/typed` |
| `.ts/import-attributes` | token attributes for import-attribute pattern nodes outside the code layer; imported by the modules that read them |
| `.ts/names` | JS/TS scopes, declarations, references and binding attributes over generic name queries; imported by `.ts` |
| `.ts/literals` | shared `literal_value` and `static_name` attributes over literal syntax and generic JavaScript coercion; imported by `.ts` |
| `.ts/effects` | JS/TS evaluation order, separate bodies and control, write and suspension effects; imported by `.ts` |
| `.ts/values` | JS/TS value and type declarations (`unwraps`, `fits`) and traits for `could` / `always`; imported by `.ts` |
| `.ts/values/arguments` | implicit `arguments` values; explicit shadows and module-top names unchanged; imported by `.ts/values` |
| `.ts/values/inspection` | shared value traits, unsafe categories and diagnostic type display; imported by `.ts/values` |
| `.ts/values/enums` | enum type and runtime views, real declaration origins, member values and widening; imported by `.ts/values` |
| `.ts/calls` | JS/TS call and construct signatures, invocation arguments, result annotations and parameter contracts; imported by `.ts` |
| `.ts/modules` | JS/TS publications, import links, namespaces, globals and module-resolution profiles; imported by `.ts` |
| `.ts/globals` | lint environment globals: per-edition builtin lists, CommonJS globals, configured settings and `/* global */` comment entries; imported by `.ts` |
| `.ts/suggestions/globals` | configured-global helpers: global value aliases and methods, constructor/prototype policy, `Object.hasOwn`, label visibility; imported by `.ts` |
| `.ts/suggestions/accessors` | accessor facts: `accessor_pair`, `property_descriptor`, `accessor`, `enclosing_accessor`; imported by `.ts` |
| `.ts/directives` | ECMAScript edition, source environment, directive prologues and strict-code policy over the lint environment; imported by `.ts` |
| `.ts/constants` | constant-expression evaluation and policy helpers for JS/TS; imported by `.ts` |
| `.ts/narrowing` | JS/TS narrowing: where tests apply (`propagates`) and what they prove (`truthy`, `excludes`, `includes`, `has`); imported by `.ts` |
| `.ts/scoping/atomic` | read–suspension–write race policy over the shared execution graph; imported by `.ts/scoping` |
| `.ts/scoping/bindings` | shared binding, lexical execution context and reference helpers; imported by `.ts/scoping` |
| `.ts/scoping/constness` | prefer-const policy over bindings and writes; imported by `.ts/scoping` |
| `.ts/scoping/globals` | implicit and redeclared global policy over the lint environment; imported by `.ts/scoping` |
| `.ts/scoping/shadowing` | shadowing and redeclaration policy over opaque scopes and the environment; imported by `.ts/scoping` |
| `.ts/scoping/loops` | iteration captures and grouped/unmodified loop condition policy; imported by `.ts/scoping` |
| `.ts/scoping/receivers` | lexical receiver contexts, receiver-bound destinations and own-body receiver use; imported by `.ts/scoping` |
| `.ts/scoping/usage` | unused binding policy (`unused_var`, `unused_globals`) over `name_binding` declarations and references; imported by `.ts/scoping` |
| `.ts/regex` | declared regexp grammar and mapped literal/constructor views, validation and finding policy; imported by `.ts` |
| `.ts/regex/grammar` | regexp pattern grammar; flag-selected Unicode and Unicode-set goals |
| `.ts/regex/values` | shared string decoding, source maps and semantic regexp views |
| `.ts/regex/checks` | regexp validation and constructor error policy |
| `.ts/regex/suggestions` | regexp escape, unnamed capture and Unicode flag policy |
| `.ts/regex/findings` | control characters, empty classes, space runs, backreferences and misleading class sequences |
| `.ts/parentheses` | expression/type parenthesis policy and IIFE wrapping, imported by `.ts/structure` |
| `.ts/structure/padding` | statement padding policies and comment-aware blank-line fixes, imported by `.ts/structure` |
| `.ts/spreads` | type-aware spread policy and allowed-type options, imported by `.ts/typed`; suggestions remain diagnostic-only |
| `.ts/typed/assignments` | unsafe assignment, destructuring and spread policy; imported by `.ts/typed` |
| `.ts/typed/enums` | number/string enum comparison policy, shared enum and union exemptions, including switch cases; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/includes` | membership tests with compatible `indexOf`/`includes` methods, literal regex tests and comment-safe fixes; imported by `.ts/typed` |
| `.ts/typed/qualifiers` | redundant runtime namespace qualifiers whose unqualified name binds the same declaration, with comment-safe fixes (type qualifiers not yet); imported by `.ts/typed` |
| `.ts/typed/switches` | literal, boolean, enum and unique-symbol switch exhaustiveness, default policies and trailing-comment exemptions; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/type-parameters` | unnecessary formal type parameters in known scalar, array and tuple signature graphs; opaque graphs and inherited classes skipped; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/awaitability` | own async syntax (`await`, `for await`, `await using`) and Values decisions for `es-require-await`'s missing-await check; imported by `.ts/typed` |
| `.ts/typed/unbound-method` | receiver-preserving method extraction, including destructuring and ignoreStatic; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/type-arguments` | redundant explicit type arguments (`es-no-unnecessary-type-arguments`) equal to their defaults, with angle-preserving removal fixes; reduce assertions moved to call type parameters (`es-prefer-reduce-type-parameter`); imported by `.ts/typed` |
| `.ts/scoping/member-uses` | private-member use policy over lexical names, contextual receivers and declared write targets; imported by `.ts/problems` |
| `.ts/problems/numeric-precision` | numeric-literal precision policy over generic number formatting; imported by `.ts/problems` |
| `.ts/typed/strict-void-return` | non-void values flowing into void-returning callback contexts, with `allowReturnAny`; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/regexp-exec` | non-global string matching policy and fixes (`es-prefer-regexp-exec`); imported by `.ts/typed` |
| `.ts/typed/for-in-arrays` | array-like `for-in` policy (`es-no-for-in-array`); imported by `.ts/typed` |
| `.ts/typed/promises` | `es-no-misused-promises` variants: `conditionals`, `predicates` (array callbacks), `spreads`, `void-arguments`, `void-attributes`, `void-inherited-methods`, `void-properties`, `void-returns`, `void-variables`; imported by `.ts/typed`, diagnostic-only |
| `.ts/typed/errors` | Error ancestry, any/unknown/undefined categories and caught/rejection-binding provenance; imported by `.ts/typed` |
| `.ts/enums` | enum transfer policy (`es-no-unsafe-enum-assignment`) over the `type_flow` fact, imported by `.ts` |
| `.ts/assertions` | assertion syntax, type-aware assertion and non-null policy, imported by `.ts` and `.ts/style` |
| `.ts/naming` | name-format policy (`es-naming-convention` variants), open context groups and shared `naming_failure` helper; imported by `.ts/scoping` |
| `.ts/deprecation` | declaration documentation policy (`es-no-deprecated`), imported by `.ts`; diagnostic-only |
| `.ts/import-no-deprecated` | `import/no-deprecated`: deprecated exports, their import specifiers, uses and namespace members, imported by `.ts/typed`; diagnostic-only |
| `.ts/type-specifiers` | shared type/value origin matching helpers (`matches_type_specifier`, `type_is_allowed`) |
| `.ts/methods` | type-aware method policy (`es-prefer-find`), imported by `.ts/typed`; filter result zero-index/bracket/at access, computed constant methods, sequences and both conditional branches; diagnostic-only |
| `.ts/selectors` | open `restricted_syntax` group and prohibition rule (`es-no-restricted-syntax`), imported by `.ts` |
| `.ts/problems/helpers` | shared problem state and macros (`ts_literal`, `is_global`, `is_real_global`, `is_write`, `problem_expression_value_used`); imported by `.ts/problems` |
| `.ts/problems/declarations` | writes to bindings, undefined names and unused assignments; imported by `.ts/problems` |
| `.ts/problems/imports` | import writes, unused and unassigned imports; imported by `.ts/problems` |
| `.ts/problems/control-flow` | loops, branches, reachability, returns and switch fallthrough; imported by `.ts/problems` |
| `.ts/problems/duplicates` | duplicate object keys, class members, parameters and enum values; imported by `.ts/problems` |
| `.ts/problems/expressions` | comparison, condition, assignment and literal mistakes; imported by `.ts/problems` |
| `.ts/problems/classes` | `super()` ordering, `this`-before-`super` and setter returns; imported by `.ts/problems` |
| `.ts/problems/statements` | debugger, empty patterns and blocks, octal literals and array holes; imported by `.ts/problems` |
| `.ts/problems/calls` | impossible calls, async Promise executors and borrowed prototype methods; imported by `.ts/problems` |
| `.ts/problems/regexp` | regular expression validation rules; imported by `.ts/problems` |
| `.ts/problems/typescript` | `@ts-` directives, `Function` types and empty interfaces; imported by `.ts/problems` |
| `.ts/problems/non-null` | non-null assertions and unsafe optional chains; imported by `.ts/problems` |
| `.ts/problems/whitespace` | irregular whitespace and unexpected newlines; imported by `.ts/problems` |
| `.ts/suggestions/helpers` | shared suggestion state and value helpers; imported by `.ts/suggestions` |
| `.ts/suggestions/control-flow` | loops, branches, statements and clauses; imported by `.ts/suggestions` |
| `.ts/suggestions/functions` | function forms, bindings, bodies and calls; imported by `.ts/suggestions` |
| `.ts/suggestions/constructors` | constructors and native prototype extension; imported by `.ts/suggestions` |
| `.ts/suggestions/expressions` | sequences, comparisons, concatenation, ternaries and coercions; imported by `.ts/suggestions` |
| `.ts/suggestions/errors` | thrown values, useless catches and preserved caught errors; imported by `.ts/suggestions` |
| `.ts/suggestions/strings` | string content: escapes, script urls and line continuation; imported by `.ts/suggestions` |
| `.ts/suggestions/regexp` | regular expression literal preferences; imported by `.ts/suggestions` |
| `.ts/suggestions/builtins` | built-in globals, prototypes and their aliases; imported by `.ts/suggestions` |
| `.ts/suggestions/directives` | `use strict` directive policy; imported by `.ts/suggestions` |
