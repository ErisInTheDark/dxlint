//! Facts native front ends answer about a node (`uses`, `type`, ...): one
//! registry entry each, read like built-in variables. On layers without a
//! native front end they are null.
//!
//! The registry is this file's [`CORE`] list plus each topic module's own
//! list ([`crate::native::ts::FACT_GROUPS`]), so topics add facts without
//! touching a shared file.

use std::sync::LazyLock;

use crate::native::ts::facts::Facts;

use super::Value;

/// A fact. Topic modules read the file directly through
/// [`Facts::with_file`]: `get: |f, n| f.with_file(my_fact, n)` with
/// `fn my_fact(file: &NativeFile, node: u32) -> Value<'static>`.
pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub get: fn(&dyn Facts, u32) -> Value<'static>,
}

thread_local! {
    static ARGS: std::cell::RefCell<Vec<Value<'static>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `get` with `args` as its arguments: a fact written as a call
/// (`member_order(groups, order)`) reads them with [`args`]. For analyses
/// whose whole-file state depends on a rule's options.
pub fn with_args(args: Vec<Value<'static>>, get: impl FnOnce() -> Value<'static>) -> Value<'static> {
    let saved = ARGS.with(|a| std::mem::replace(&mut *a.borrow_mut(), args));
    let out = get();
    ARGS.with(|a| *a.borrow_mut() = saved);
    out
}

/// The arguments of the fact call being evaluated (empty when the fact is
/// read as a name).
pub fn args() -> Vec<Value<'static>> {
    ARGS.with(|a| a.borrow().clone())
}

/// `v`'s value, or null.
pub fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> Value<'static>) -> Value<'static> {
    v.map_or(Value::Null, f)
}

/// A list of words.
pub fn words(list: Vec<&'static str>) -> Value<'static> {
    Value::List(list.into_iter().map(Value::from).collect())
}

/// Every fact: the core list, then each topic's.
pub static FACTS: LazyLock<Vec<&'static Fact>> = LazyLock::new(|| CORE.iter().chain(crate::native::ts::FACT_GROUPS.iter().flat_map(|g| g.iter())).collect());

const CORE: &[Fact] = &[
    Fact {
        name: "uses",
        doc: "native layers: how often the name this identifier declares is used (null if it declares nothing)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.uses as usize)),
    },
    Fact {
        name: "value_uses",
        doc: "native layers: uses that need the runtime value (not type positions, `typeof x` in types)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.value_uses as usize)),
    },
    Fact {
        name: "self_uses",
        doc: "native layers: those uses inside the declaration itself (a function calling itself)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.self_uses as usize)),
    },
    Fact {
        name: "writes",
        doc: "native layers: assignments to the name this identifier declares",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.writes as usize)),
    },
    Fact {
        name: "declaration_kind",
        doc: "native layers: what this identifier's declaration is (`const`, `parameter`, `import`, `class`, `type`, ...)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.kind)),
    },
    Fact { name: "exported", doc: "native layers: the declaration is `export`ed", get: |f, n| opt(f.binding(n), |b| Value::Bool(b.exported)) },
    Fact {
        name: "ambient",
        doc: "native layers: the declaration is inside `declare` (no runtime code)",
        get: |f, n| opt(f.binding(n), |b| Value::Bool(b.ambient)),
    },
    Fact {
        name: "imported_from",
        doc: "native layers: the module an identifier's binding is imported from (null if not imported)",
        get: |f, n| opt(f.import_of(n), |(from, _)| Value::from(from)),
    },
    Fact {
        name: "declaration_doc",
        doc: "native layers: the JSDoc of the declaration an identifier is or refers to, followed into imported files",
        get: |f, n| opt(f.decl_doc(n), Value::from),
    },
    Fact {
        name: "type",
        doc: "native layers: the expression's declared type as text (`Promise<void>`, `never`); null when it cannot tell",
        get: |f, n| opt(f.type_text(n), Value::from),
    },
    Fact {
        name: "type_flags",
        doc: "native layers: what the expression's type is made of: `any`, `never`, `string`, `number`, `boolean`, `bigint`, `symbol`, `literal`, `enum`, `array`, `tuple`, `promise`, `function`, `object`, `null`, `undefined`, `nullable`, `union`, and `unknown` for parts zlint cannot read; null when nothing is known",
        get: |f, n| opt(f.type_flags(n), words),
    },
    Fact {
        name: "type_members",
        doc: "native layers: member names of the expression's type, inherited ones included (`then`, `[Symbol.dispose]`)",
        get: |f, n| opt(f.type_members(n), |names| Value::List(names.into_iter().map(|n| Value::from(n.to_string())).collect())),
    },
    Fact {
        name: "type_parameter_defaults",
        doc: "native layers: on a call, `new`, type reference or class: each type parameter's default, printed (null where none)",
        get: |f, n| opt(f.type_params(n), |(defaults, _)| Value::List(defaults.into_iter().map(|t| opt(t, Value::from)).collect())),
    },
    Fact {
        name: "type_parameters_inferred",
        doc: "native layers: on a call or `new`: what the arguments would infer for each type parameter (null where unknown)",
        get: |f, n| opt(f.type_params(n), |(_, inferred)| Value::List(inferred.into_iter().map(|t| opt(t, Value::from)).collect())),
    },
    Fact {
        name: "type_argument_redundant",
        doc: "native layers: on the last type argument of a call, `new`, type reference or class: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types that are assignable both ways count as the same); null otherwise",
        get: |f, n| opt(f.type_arg_redundant(n), Value::from),
    },
    Fact {
        name: "resolved",
        doc: "native layers: what a referencing identifier resolves to: `local`, `import`, or `global` (declared nowhere in the file); null for declarations and type names",
        get: |f, n| opt(f.resolved(n), Value::from),
    },
    Fact {
        name: "global",
        doc: "native layers: on an identifier referring to a global: how eslint configures it, `readonly` (the ECMAScript built-ins, `/* global x */`), `writable` (`/* global x: writable */`) or `off` (`/* global x: off */`); null when nothing declares it (or it is not a global)",
        get: |f, n| opt(f.global(n), Value::from),
    },
    Fact {
        name: "access",
        doc: "native layers: on a referencing identifier: how it uses the name: `read`, `write` (`x = 1`, destructuring, `for (x of ...)`), `read_write` (`x += 1`, `x++`) or `type`",
        get: |f, n| opt(f.access(n), Value::from),
    },
    Fact {
        name: "write_site",
        doc: "native layers: on a written identifier or member (`x`, `a.b`), or an array or object literal that is a destructuring target (`[a, b] = c`): the start offset of what writes it (the assignment, update, `delete` or `for` statement), for `report at`; null otherwise",
        get: |f, n| opt(f.write_site(n), Value::from),
    },
    Fact {
        name: "static_name",
        doc: "native layers: on a property, method, class or enum member, `a.b` or `a[\"b\"]`: its name when known without running anything (`a`, `\"a\"` -> `a`, `0x1` -> `1`, `[`k`]` -> `k`; null for `#private` names), as eslint's `getStaticPropertyName`; on a literal: its value as text (`'\\x41'` -> `A`); null otherwise",
        get: |f, n| opt(f.static_name(n), Value::from),
    },
    Fact {
        name: "global_value",
        doc: "native layers: on an expression: the global it stands for, followed as eslint's ReferenceTracker does: `Math`, `globalThis.Math`, `(Math)`, a variable only ever holding it (`const m = Math`), or a branch of `?:` / `||` that is one; null otherwise",
        get: |f, n| opt(f.global_value(n), Value::from),
    },
    Fact {
        name: "declaration_kinds",
        doc: "native layers: on a declaring identifier: the kinds of every declaration merged with it (`class` and `interface` of one name: `[\"class\", \"interface\"]`)",
        get: |f, n| opt(f.declaration_kinds(n), words),
    },
    Fact {
        name: "literal_value",
        doc: "native layers: on a literal (also `-1`, `+1`, parenthesised): `{ type, value }` with the JavaScript type (`string`, `number`, `bigint`, `boolean`, `null`, `regex`) and the value as text (`-0` for negative zero); null otherwise",
        get: |f, n| opt(f.literal_value(n), |(t, v)| Value::Record(vec![("type".into(), Value::from(t)), ("value".into(), Value::from(v))])),
    },
    Fact {
        name: "constant_truthiness",
        doc: "native layers: on an expression: its truthiness can never change (eslint's `isConstant` in a boolean position: `true`, `{}`, `x || true`, `void x`)",
        get: |f, n| Value::Bool(f.is_constant(n, true)),
    },
    Fact {
        name: "constant_value",
        doc: "native layers: on an expression: its value as a string or number can never change (eslint's `isConstant` outside boolean positions)",
        get: |f, n| Value::Bool(f.is_constant(n, false)),
    },
    Fact {
        name: "leaves_finally",
        doc: "native layers: on `return`, `throw`, `break` or `continue`: it jumps out of a `finally` block (not just out of a loop, switch or label inside it)",
        get: |f, n| Value::Bool(f.leaves_finally(n)),
    },
    Fact {
        name: "contains_yield",
        doc: "native layers: a `yield` inside the node, outside nested functions (on a `for` loop: outside its initializer)",
        get: |f, n| Value::Bool(f.contains_yield(n)),
    },
    Fact {
        name: "unused_var",
        doc: "native layers: on a variable's first declaring identifier, no-unused-vars' view of it (eslint's in `.js`, typescript-eslint's in `.ts`): `used`, `exported`, `silent` (never reported), `def` (`variable`, `parameter`, `catch`: which ignore pattern applies), `array_element`, `array_target`, `setter`, `before_used_param` (\"after-used\" keeps it), `rest_sibling`, `static_block`, `using`, `global` (script top level), `action` (`defined` or `assigned a value`), `only_type`, `offset` (where to report) and `ignored_offset` (where \"marked as ignored but is used\" points); null elsewhere",
        get: |f, n| f.with_file(crate::native::ts::unused::var_fact, n),
    },
    Fact {
        name: "useless_assignment",
        doc: "native layers: on an identifier being assigned (`x = 1`, `let x = 1`, `x++`), true when no code path reads that value before it is overwritten or its function ends (eslint's no-useless-assignment, with its exceptions: variables nothing reads, read from other functions, exported, or assigned in a `try` block)",
        get: |f, n| f.with_file(crate::native::ts::dead_stores::fact, n),
    },
    Fact {
        name: "unused_globals",
        doc: "native layers: on the program, `/* global */` comment names nothing reads, as records `{ name, offset }` (offset of the name in the comment)",
        get: |f, n| f.with_file(crate::native::ts::unused::globals_fact, n),
    },
    Fact {
        name: "source_type",
        doc: "native layers: the file's eslint `sourceType`: `commonjs` (`.cjs`, `.cts`), `module` (`.mjs`, `.mts`, or any file that imports or exports) or `script`; the same on every node",
        get: |f, n| f.with_file(|file, _| Value::from(file.source_type()), n),
    },
    Fact {
        name: "unparen_offset",
        doc: "native layers: the offset of the node with the parentheses around it removed (where eslint, which has no parenthesis nodes, reports it), for `report at`",
        get: |f, n| opt(f.unparen_offset(n), Value::from),
    },
    Fact {
        name: "unexpected_multiline",
        doc: "native layers: on a call, `a[b]`, tagged template or `a / b / g`: when a line break before its `(`, `[`, template or `/` lets the next line continue the expression (eslint's no-unexpected-multiline): `{ offset, kind }` with kind `function`, `property`, `taggedTemplate` or `division`",
        get: |f, n| opt(f.unexpected_multiline(n), |(at, kind)| Value::Record(vec![("offset".into(), Value::from(at)), ("kind".into(), Value::from(kind))])),
    },
    Fact {
        name: "optional_chain_risks",
        doc: "native layers: the optional chains whose short-circuit `undefined` this node would trip on (eslint's no-unsafe-optional-chaining: called, destructured, spread, `in`, `extends`, `for of`...; also through `&&`, `||`, `??`, `?:`, `await`), as records `{ offset, arithmetic }` (`arithmetic` for `+`, `-`, `*`, ... that make `NaN`)",
        get: |f, n| {
            Value::List(
                f.optional_chain_risks(n)
                    .into_iter()
                    .map(|(at, arith)| Value::Record(vec![("offset".into(), Value::from(at)), ("arithmetic".into(), Value::Bool(arith))]))
                    .collect(),
            )
        },
    },
    Fact {
        name: "self_assignments",
        doc: "native layers: on an assignment (`=`, `&&=`, `||=`, `??=`): what it assigns to itself, also inside destructuring (`[a, b] = [a, b]`), as records `{ offset, name, member }` (`member` for `a.b = a.b`)",
        get: |f, n| {
            opt(f.self_assignments(n), |list| {
                Value::List(
                    list.into_iter()
                        .map(|(at, name, member)| {
                            Value::Record(vec![("offset".into(), Value::from(at)), ("name".into(), Value::from(name)), ("member".into(), Value::Bool(member))])
                        })
                        .collect(),
                )
            })
        },
    },
    Fact {
        name: "conditional_ancestor",
        doc: "native layers: the layer of the `if`, loop or `?:` whose test holds the node (`ts_if`, `ts_while`, `ts_do_while`, `ts_for`, `ts_conditional`), not across functions; null outside tests",
        get: |f, n| opt(f.conditional_ancestor(n), Value::from),
    },
    Fact {
        name: "duplicate_condition",
        doc: "native layers: on an `if`: an earlier branch of its `else if` chain already covers its test (eslint's no-dupe-else-if: by tokens, `&&`/`||` in any order, `a` covers `a && b`)",
        get: |f, n| opt(f.duplicate_condition(n), Value::Bool),
    },
    Fact {
        name: "duplicate_case",
        doc: "native layers: on a `case`: an earlier case of its switch has the same test (by tokens, whatever the spacing and comments)",
        get: |f, n| opt(f.duplicate_case(n), Value::Bool),
    },
    Fact {
        name: "wrong_direction",
        doc: "native layers: on a `for`: its update moves the counter away from where its test ends the loop (eslint's for-direction; steps are evaluated statically: `i -= -1`, `i += 1n`)",
        get: |f, n| opt(f.wrong_direction(n), Value::Bool),
    },
    Fact {
        name: "constant_comparison",
        doc: "native layers: on a binary or logical expression: what eslint's no-constant-binary-expression finds, `{ kind, offset, detail }`: `constantShortCircuit` (detail `truthiness` / `nullishness`), `constantBinaryOperand` (detail: the other side, `left` / `right`), `alwaysNew`, `bothAlwaysNew`, at the operand it blames; null when nothing is constant",
        get: |f, n| {
            opt(f.constant_comparison(n, false), |(kind, at, detail)| {
                Value::Record(vec![("kind".into(), Value::from(kind)), ("offset".into(), Value::from(at)), ("detail".into(), Value::from(detail))])
            })
        },
    },
    Fact {
        name: "constant_relation",
        doc: "native layers: on a binary expression: `<`, `<=`, `>`, `>=` between two literals (no-constant-binary-expression's `checkRelationalComparisons`)",
        get: |f, n| Value::Bool(f.constant_comparison(n, true).is_some_and(|(kind, ..)| kind == "constantRelationalComparison")),
    },
    Fact {
        name: "accessor",
        doc: "native layers: on a function, arrow or method: `get` or `set` when it is a getter or setter, or the `get` / `set` of a property descriptor (`Object.defineProperty(o, k, { get() {} })`); null otherwise",
        get: |f, n| opt(f.accessor(n), Value::from),
    },
    Fact {
        name: "enclosing_accessor",
        doc: "native layers: `accessor` of the nearest function, arrow or method around the node",
        get: |f, n| opt(f.enclosing_accessor(n), Value::from),
    },
    Fact {
        name: "duplicate_parameters",
        doc: "native layers: on a function, arrow or method: the names its parameters declare more than once",
        get: |f, n| opt(f.duplicate_parameters(n), |names| Value::List(names.into_iter().map(Value::from).collect())),
    },
    Fact {
        name: "imported_name",
        doc: "native layers: the name an identifier's binding imports (`default`, `*` for a namespace, or the export's name); null if not imported",
        get: |f, n| opt(f.import_of(n), |(_, name)| Value::from(name)),
    },
    Fact {
        name: "reference_kind",
        doc: "native layers: on a referencing identifier: the kind of the declaration it resolves to (`const`, `parameter`, `catch`, ...; see `declaration_kind`)",
        get: |f, n| opt(f.ref_kind(n), Value::from),
    },
    Fact {
        name: "member_reads",
        doc: "native layers: on a class property, method or parameter property: reads through `this` (or `x.#name`) inside the class",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.reads as usize)),
    },
    Fact {
        name: "member_writes",
        doc: "native layers: on a class member: writes through `this` (or `x.#name`) outside the constructor (`=`, `+=`, `++`, `delete`, destructuring)",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.writes as usize)),
    },
    Fact {
        name: "member_constructor_writes",
        doc: "native layers: on a class member: writes inside the constructor",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.ctor_writes as usize)),
    },
    Fact {
        name: "uses_this",
        doc: "native layers: on a function or method: its own body uses `this` (arrow functions inside count, nested functions do not)",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_THIS != 0)),
    },
    Fact {
        name: "uses_arguments",
        doc: "native layers: on a function or method: its own body uses `arguments`",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_ARGUMENTS != 0)),
    },
    Fact {
        name: "has_yield",
        doc: "native layers: on a function or method: its own body contains `yield`",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::HAS_YIELD != 0)),
    },
    Fact {
        name: "super_calls",
        doc: "native layers: on the constructor of a class with `extends`: `all`, `some` or `none` of the paths that leave it normally (falling off the end, `return`) call `super()` (throwing paths need none; `return value` stands in for it)",
        get: |f, n| opt(f.super_calls(n), Value::from),
    },
    Fact {
        name: "code_path",
        doc: "native layers: what code path analysis (eslint's) says about the node: on a function, method, arrow, static block or the program: `falls_off` (its end is reachable) or `returns`; on a `case`: `falls_through` (its statements' end is reachable), `leaves` or `unreachable`; on a statement: `unreachable_run` (the first of a run of unreachable statements; see `unreachable_run_end`) or `unreachable`; on `this` / `super.x` in a derived constructor: `before_super` (some path has not called `super()` yet); on a `super()` call: `duplicate_super` (some path called it already) or `bad_super` (the class extends something that cannot be a constructor); null otherwise",
        get: |f, n| opt(f.code_path(n), Value::from),
    },
    Fact {
        name: "unused_member",
        doc: "native layers: on a class member's name (a key, or a parameter property's name): nothing reads it, as typescript-eslint's class scope analyzer counts (writes only count for accessors). The member's kind: `hash` (`#x`), `private` (TypeScript `private`) or `public`; null when it is read",
        get: |f, n| opt(f.unused_member(n), Value::from),
    },
    Fact {
        name: "function_name",
        doc: "native layers: on a function, arrow or method: how eslint names it in messages (`function 'f'`, `arrow function`, `static getter 'x'`, `private method #y`, `constructor`)",
        get: |f, n| opt(f.function_name_with_kind(n), Value::from),
    },
    Fact {
        name: "enclosing_function_name",
        doc: "native layers: `function_name` of the nearest function, arrow or method around the node",
        get: |f, n| opt(f.enclosing_function_name(n), Value::from),
    },
    Fact {
        name: "function_head",
        doc: "native layers: on a function, arrow or method: the offset eslint reports it at (its start, its property's start, or an arrow's `=>`), for `report at`",
        get: |f, n| opt(f.function_head(n), Value::from),
    },
    Fact {
        name: "return_values",
        doc: "native layers: on a function, arrow or method: whether `all` or `some` of its own `return`s give a value; `bare` when all are `return;`, `none` when it has none",
        get: |f, n| opt(f.return_values(n), Value::from),
    },
    Fact {
        name: "unreachable_run_end",
        doc: "native layers: on a statement that starts a run of unreachable statements: the offset where the run ends",
        get: |f, n| opt(f.unreachable_run_end(n), Value::from),
    },
    Fact {
        name: "regex_findings",
        doc: "native layers: on a regex literal, or a call of the global `RegExp` (aliases too) with a pattern known statically: what eslint's regex rules find, as records `{ check, kind, data, offset, end }`. `check`: `empty_class` (literals), `control` (data: `\\x1f, \\x1e`), `spaces` (kind: how many), `backref` (kind: `nested`, `forward`, `backward`, `disjunctive`, `intoNegativeLookaround`; data: backreference, group, ` and another group`), `misleading` (kind: `surrogatePairWithoutUFlag`, `surrogatePair`, `combiningClass`, `emojiModifier`, `regionalIndicatorSymbol`, `zwj`) and `misleading_escaped` (what remains with escapes allowed). `offset`/`end`: where it is written, or the pattern argument when the pattern is computed",
        get: |f, n| {
            opt(f.regex_findings(n), |list| {
                Value::List(
                    list.into_iter()
                        .map(|r| {
                            Value::Record(vec![
                                ("check".into(), Value::from(r.check)),
                                ("kind".into(), Value::from(r.kind)),
                                ("data".into(), Value::List(r.data.into_iter().map(Value::from).collect())),
                                ("offset".into(), Value::from(r.offset)),
                                ("end".into(), Value::from(r.end)),
                            ])
                        })
                        .collect(),
                )
            })
        },
    },
    Fact {
        name: "regex_pattern",
        doc: "native layers: on a regex site (see `regex_findings`): `{ flags, written, offset }`: its static flags (null when unknown), how the pattern is written (`regex`, `string`, `template` or `computed`) and where",
        get: |f, n| {
            opt(f.regex_site(n), |(flags, written, at)| {
                Value::Record(vec![("flags".into(), opt(flags, Value::from)), ("written".into(), Value::from(written)), ("offset".into(), Value::from(at))])
            })
        },
    },
    Fact {
        name: "regex_pattern_error",
        doc: "native layers: on a call of the global `RegExp` with a string literal pattern: its syntax error (`Invalid regular expression: /p/u: message`); with flags that are not a string literal, only an error in every mode",
        get: |f, n| opt(f.regex_pattern_error(n), Value::from),
    },
    Fact {
        name: "regex_flags",
        doc: "native layers: on a call of the global `RegExp`: its flags when written as a string literal (`\"\"` without any); null otherwise",
        get: |f, n| opt(f.regex_call_flags(n).flatten(), Value::from),
    },
    Fact {
        name: "precision_lost",
        doc: "native layers: on a number literal: its value is not exactly representable as a double",
        get: |f, n| opt(f.precision_lost(n), Value::Bool),
    },
    Fact {
        name: "compiler_option",
        doc: "native layers: `compiler_option(\"name\")`: a boolean tsconfig compiler option for this file (`strict` counts for the options it implies); false when unset",
        get: |f, n| f.with_file(|file, _| {
            let name = args().first().map(|v| v.text().into_owned()).unwrap_or_default();
            let id = file.project.file_id(file.path);
            Value::Bool(file.project.strict_option(id, &name))
        }, n),
    },
];

use crate::native::ts::facts as facts_bits;

/// A fact's index in [`FACTS`].
pub fn fact(name: &str) -> Option<usize> {
    FACTS.iter().position(|f| f.name == name)
}
