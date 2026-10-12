use super::*;
use crate::native::ts::types::Callable;
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    static CONTEXTUAL_RETURN_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DECLARED_TARGET_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Contract {
    origin: &'static str,
    sender: Ty,
    receiver: Ty,
    declared_receiver: Option<Ty>,
    heritage: Option<u32>,
    path: Vec<String>,
}

struct Projection {
    remaining: usize,
    complete: bool,
    rows: Vec<Contract>,
}

pub(super) fn contracts(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let args = crate::expr::facts::args();
    let node_id = |value: &Value<'_>| match value { Value::Node(node, _) => Some(*node), _ => None };
    let site = args.first().and_then(node_id).unwrap_or(node);
    let source = args.get(1).and_then(node_id).unwrap_or(node);
    let path: Vec<_> = args.get(2).map(|value| value.items().iter().map(ToString::to_string).collect()).unwrap_or_default();
    let arguments = file.argument_contract_types(source);
    let requested = |name| args.get(3).is_some_and(|fields|
        fields.items().iter().any(|field| matches!(field, Value::Str(field) if field.as_ref() == name)));
    let argument_source = arguments.is_some();
    let declared = (requested("declared_target") && argument_source).then(|| {
        #[cfg(test)]
        DECLARED_TARGET_QUERIES.with(|count| count.set(count.get() + 1));
        file.declared_argument_contract_types(source)
    });
    let mut projection = Projection { remaining: 4096, complete: !matches!(&arguments, Some(Err(()))), rows: Vec::new() };
    if let Some(Ok(pairs)) = arguments {
        let targets = declared.as_ref().and_then(|answer| answer.as_ref()).and_then(|answer| answer.as_ref().ok())
            .filter(|targets| targets.len() == pairs.len() && targets.iter().zip(&pairs).all(|((left, _), (right, _))| left.same_identity(right)));
        for (index, (sender, receiver)) in pairs.into_iter().enumerate() {
            let declared_receiver = targets.and_then(|targets| targets.get(index)).map(|(_, receiver)| receiver.clone());
            projection.at_path(file, Contract { origin: "argument", sender, receiver, declared_receiver, heritage: None, path: Vec::new() }, &path);
        }
    } else if let Some(pairs) = file.return_types(source) {
        for (sender, receiver) in pairs {
            projection.at_path(file, Contract { origin: "return", sender, receiver, declared_receiver: None, heritage: None, path: Vec::new() }, &path);
        }
    } else {
        let sender = source_type(file, source);
        let (origin, mut receiver) = file.receiver_contract(site, source);
        if matches!(file.syntax.kind(site), Some(Kind::Method | Kind::Prop)) {
            if let Some(member) = file.object_member_contract(site) { receiver = member }
        }
        projection.at_path(file, Contract { origin, sender: sender.clone(), receiver, declared_receiver: None, heritage: None, path: Vec::new() }, &path);
        if file.syntax.is(site, Kind::Declarator) {
            let kind = file.syntax.parent(site).and_then(|owner| file.syntax.word(owner, Field::Kind));
            if matches!(kind, Some("using" | "await using")) {
                let receiver = file.typing().checker.global_named("Disposable", Vec::new());
                projection.at_path(file, Contract { origin: "resource", sender: sender.clone(), receiver, declared_receiver: None, heritage: None, path: Vec::new() }, &path);
            }
        }
        if matches!(file.syntax.kind(site), Some(Kind::Method | Kind::MethodSig | Kind::ClassProp | Kind::PropSig)) {
            for (heritage, receiver) in file.inherited_contracts(site) {
                projection.at_path(file, Contract { origin: "heritage", sender: sender.clone(), receiver, declared_receiver: None, heritage: Some(heritage), path: Vec::new() }, &path);
            }
        }
    }
    let contextual = (!projection.rows.is_empty() && requested("contextual_returns"))
        .then(|| contextual_returns(file, source, &path));
    Value::list(projection.rows.into_iter().map(|contract| {
        let target = returns(file, &contract.receiver);
        let declared_target = requested("declared_target").then(|| if argument_source {
            contract.declared_receiver.as_ref().and_then(|receiver| file.typing().checker.call_signature_returns(receiver))
                .map_or(Value::Null, |types| return_values(file, types))
        } else if contract.receiver.is_opaque() { Value::Null } else { target.clone() });
        let mut row = vec![
            ("origin".into(), Value::from(contract.origin)),
            ("source".into(), returns(file, &contract.sender)),
            ("target".into(), target),
            ("heritage".into(), contract.heritage.map_or(Value::Null, |node| Value::Node(node, file.syntax.text(node).to_owned().into()))),
            ("member_path".into(), Value::list(contract.path.into_iter().map(Value::from).collect())),
            ("complete".into(), Value::Bool(projection.complete)),
        ];
        if let Some(contextual) = &contextual { row.push(("contextual_returns".into(), contextual.clone())); }
        if let Some(declared_target) = declared_target { row.push(("declared_target".into(), declared_target)); }
        Value::record(row)
    }).collect())
}

fn contextual_returns(file: &NativeFile<'_>, source: u32, path: &[String]) -> Value<'static> {
    #[cfg(test)]
    CONTEXTUAL_RETURN_QUERIES.with(|count| count.set(count.get() + 1));
    let checker = &file.typing().checker;
    let mut context = file.contextual_type_of(source);
    for name in path { context = checker.member(&context, name); }
    checker.call_signature_returns(&context).map_or(Value::Null, |returns|
        Value::list(returns.iter().map(|ty| super::type_structure::return_profile(ty, checker)).collect()))
}

impl Projection {
    fn at_path(&mut self, file: &NativeFile<'_>, mut contract: Contract, path: &[String]) {
        let Some(remaining) = self.remaining.checked_sub(path.len().saturating_add(1)) else {
            self.complete = false;
            return;
        };
        self.remaining = remaining;
        let checker = &file.typing().checker;
        for name in path {
            if !file.member_keys_match(&contract.sender, &contract.receiver, name, true) { return }
            contract.declared_receiver = contract.declared_receiver.take().and_then(|receiver|
                file.member_keys_match(&contract.sender, &receiver, name, true).then(|| checker.member(&receiver, name)));
            contract.sender = checker.member(&contract.sender, name);
            contract.receiver = checker.member(&contract.receiver, name);
            contract.path.push(name.clone());
        }
        if checker.has_call_signature(&contract.sender) || checker.has_call_signature(&contract.receiver) {
            self.rows.push(contract);
        }
    }
}

fn source_type(file: &NativeFile<'_>, source: u32) -> Ty {
    let syntax = &file.syntax;
    let ty = if matches!(syntax.kind(source), Some(Kind::ClassProp | Kind::PropSig)) {
        syntax.get(source, Field::Type).or_else(|| syntax.get(source, Field::Init))
            .map_or(Ty::Unknown, |value| file.constrained_type_of(value))
    } else if syntax.is(source, Kind::MethodSig) {
        Ty::Fn(Arc::new(Callable::new(
            vec![Arc::new(crate::native::ts::summary::sig(syntax, source))], Arc::new(file.env_at(source)), None,
        )))
    } else { file.constrained_type_of(source) };
    if matches!(syntax.kind(source), Some(Kind::Method | Kind::MethodSig)) && syntax.word(source, Field::Kind) == Some("get") {
        Ty::union(file.callable_return_types(&ty))
    } else { ty }
}

fn returns(file: &NativeFile<'_>, ty: &Ty) -> Value<'static> {
    return_values(file, file.callable_return_types(ty))
}

fn return_values(file: &NativeFile<'_>, types: Vec<Ty>) -> Value<'static> {
    fn arms(file: &NativeFile<'_>, ty: &Ty, values: &mut Vec<Value<'static>>) {
        if let Ty::Union(parts) | Ty::Inter(parts) = ty {
            for part in parts { arms(file, part, values) }
            return;
        }
        let kind = match ty {
            Ty::Void => "void",
            Ty::Undefined => "undefined",
            Ty::Null => "null",
            Ty::Never => "never",
            Ty::Any => "any",
            Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => "unknown",
            Ty::Prim(kind) | Ty::Lit(_, kind) => kind,
            Ty::Fn(_) => "function",
            _ => "object",
        };
        values.push(Value::record(vec![
            ("kind".into(), Value::from(kind)),
            ("thenable".into(), Value::Bool(file.typing().checker.is_thenable(ty))),
        ]));
    }
    let mut values = Vec::new();
    for ty in types { arms(file, &ty, &mut values) }
    Value::list(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    fn project(file: &NativeFile<'_>, node: u32, requested: bool) -> Value<'static> {
        let value = Value::Node(node, file.syntax.text(node).to_owned().into());
        let mut args = vec![value.clone(), value, Value::list(Vec::new())];
        if requested { args.push(Value::list(vec![Value::from("contextual_returns")])); }
        crate::expr::facts::with_args(args, || super::contracts(file, node))
    }

    fn declared(file: &NativeFile<'_>, node: u32, path: &[&str]) -> Value<'static> {
        let value = Value::Node(node, file.syntax.text(node).to_owned().into());
        crate::expr::facts::with_args(vec![
            value.clone(), value,
            Value::list(path.iter().map(|name| Value::from((*name).to_owned())).collect()),
            Value::list(vec![Value::from("declared_target")]),
        ], || super::contracts(file, node))
    }

    #[test]
    fn declared_targets_keep_callback_bounds_separate_from_inference_and_default_work() {
        with_native_file("declare function accept<Cb extends () => void>(cb: Cb): void; accept(/*@*/() => 1);", |file, marks| {
            DECLARED_TARGET_QUERIES.with(|count| count.set(0));
            let legacy = project(file, marks[0], false);
            assert_eq!(DECLARED_TARGET_QUERIES.with(|count| count.get()), 0);
            assert_eq!(legacy.items()[0].key("target").items()[0].key("kind"), Value::from("number"));
            let result = declared(file, marks[0], &[]);
            let target = result.items()[0].key("declared_target");
            assert_eq!(target.items().len(), 1, "callback declaration bound was lost to argument inference");
            assert_eq!(target.items()[0].key("kind"), Value::from("void"));
            assert_eq!(DECLARED_TARGET_QUERIES.with(|count| count.get()), 1);
            let stripped = Value::list(result.items().iter().map(|row| {
                let Value::Record(entries) = row else { panic!("expected a contract") };
                Value::record(entries.iter().filter(|(key, _)| key.as_ref() != "declared_target").cloned().collect::<Vec<_>>())
            }).collect());
            assert_eq!(stripped, legacy, "declaration projection changed inferred contracts");
            assert_eq!(project(file, marks[0], false), legacy);
            assert_eq!(DECLARED_TARGET_QUERIES.with(|count| count.get()), 1, "default caller queried declarations");
        });
    }

    #[test]
    fn declared_targets_preserve_overloads_constructors_rest_and_selected_members() {
        for (source, path, expected) in [
            ("declare function accept(cb: () => void): void; declare function accept(cb: () => number): void; accept(/*@*/() => 1);", &[][..], &["void", "number"][..]),
            ("declare class Holder { constructor(cb: () => void) } new Holder(/*@*/() => 1);", &[][..], &["void"][..]),
            ("declare class Holder<Cb extends () => void> { constructor(cb: Cb) } new Holder(/*@*/() => 1);", &[][..], &["void"][..]),
            ("declare class Holder<Cb extends () => void> { constructor(cb: Cb) } new Holder<() => number>(/*@*/() => 1);", &[][..], &["number"][..]),
            ("declare function accept<Cb extends () => void>(cb: Cb): void; accept<() => number>(/*@*/() => 1);", &[][..], &["number"][..]),
            ("declare function accept<T extends void>(...cb: (() => T)[]): void; accept(() => {}, /*@*/() => 1);", &[][..], &["void"][..]),
            ("declare function accept(...cb: [() => number, () => void]): void; accept(() => 1, /*@*/() => 1);", &[][..], &["void"][..]),
            ("declare function accept<T extends void>(object: {callback: () => T}): void; accept(/*@*/{callback: () => 1});", &["callback"][..], &["void"][..]),
            ("const callback: () => void = /*@*/() => 1;", &[][..], &["void"][..]),
        ] {
            with_native_file(source, |file, marks| {
                let result = declared(file, marks[0], path);
                assert_eq!(result.items().len(), 1, "{source}");
                let target = result.items()[0].key("declared_target");
                let kinds: Vec<_> = target.items().iter().map(|part| part.key("kind").to_string()).collect();
                assert_eq!(kinds, expected, "{source}");
            });
        }
    }

    #[test]
    fn declared_targets_do_not_turn_unavailable_or_bounded_out_contracts_into_empty_proofs() {
        let mut bounded = "interface Context { (cb: () => void): void;".to_owned();
        for index in 0..=Checker::CONTRACT_WORK_LIMIT {
            bounded.push_str(&format!(" property{index}: number;"));
        }
        bounded.push_str(" } declare const accept: Context; accept(/*@*/() => 1);");
        for source in ["declare const accept: Unknown; accept(/*@*/() => 1);", bounded.as_str()] {
            with_native_file(source, |file, marks| {
                let result = declared(file, marks[0], &[]);
                assert!(!result.items().is_empty(), "{source}");
                assert_eq!(result.items()[0].key("declared_target"), Value::Null);
            });
        }
    }

    #[test]
    fn contextual_return_profiles_preserve_unions_and_legacy_contracts_without_extra_default_work() {
        with_native_file("const callback: () => void | number = /*@*/() => 1;", |file, marks| {
            assert_eq!(file.syntax.kind(marks[0]), Some(Kind::Arrow));
            CONTEXTUAL_RETURN_QUERIES.with(|count| count.set(0));
            let legacy = project(file, marks[0], false);
            assert_eq!(CONTEXTUAL_RETURN_QUERIES.with(|count| count.get()), 0);
            let enriched = project(file, marks[0], true);
            let rows = enriched.items();
            assert!(!rows.is_empty());
            let profiles = rows[0].key("contextual_returns").items();
            assert_eq!(profiles.len(), 1, "contextual signature return was not retained");
            assert_eq!(profiles[0].key("kind"), Value::from("union"));
            let parts = profiles[0].key("parts").items();
            assert!(parts.iter().any(|part| part.key("kind") == Value::from("void")));
            assert!(parts.iter().any(|part| part.key("kind") == Value::from("number")));
            assert_eq!(CONTEXTUAL_RETURN_QUERIES.with(|count| count.get()), 1);
            let stripped = Value::list(rows.iter().map(|row| {
                let Value::Record(entries) = row else { panic!("expected a contract") };
                Value::record(entries.iter().filter(|(key, _)| key.as_ref() != "contextual_returns").cloned().collect::<Vec<_>>())
            }).collect());
            assert_eq!(stripped, legacy, "opt-in projection changed the existing contract");
            assert_eq!(project(file, marks[0], false), legacy);
            assert_eq!(CONTEXTUAL_RETURN_QUERIES.with(|count| count.get()), 1, "default caller queried extra context");
        });
    }

    #[test]
    fn contextual_return_profiles_keep_inherited_generic_callbacks_and_open_signature_parameters() {
        for (source, expected) in [
            ("interface Base<T> { (): T } interface Derived extends Base<void> {} const callback: Derived = /*@*/() => 1;", "void"),
            ("type Callback = <T>() => T; const callback: Callback = /*@*/() => 1;", "parameter"),
            ("declare function accept(callback: () => void): void; accept(/*@*/() => 1);", "void"),
            ("const object: {callback: () => void} = {callback: /*@*/() => 1};", "void"),
        ] {
            with_native_file(source, |file, marks| {
                assert_eq!(file.syntax.kind(marks[0]), Some(Kind::Arrow));
                let result = project(file, marks[0], true);
                let rows = result.items();
                assert!(!rows.is_empty(), "missing callable contract for {source}");
                let profiles = rows[0].key("contextual_returns").items();
                assert_eq!(profiles.len(), 1, "missing contextual return for {source}");
                assert_eq!(profiles[0].key("kind"), Value::from(expected), "{source}");
            });
        }
    }

    #[test]
    fn contextual_return_profiles_distinguish_explicit_unknown_and_missing_context() {
        for (source, missing) in [
            ("const callback: () => unknown = /*@*/() => 1;", false),
            ("const callback = /*@*/() => 1;", true),
        ] {
            with_native_file(source, |file, marks| {
                let result = project(file, marks[0], true);
                let profiles = result.items()[0].key("contextual_returns");
                if missing { assert_eq!(profiles, Value::Null); }
                else {
                    assert_eq!(profiles.items().len(), 1);
                    assert_eq!(profiles.items()[0].key("kind"), Value::from("unknown"));
                }
            });
        }
    }

    #[test]
    fn contextual_return_profiles_do_not_present_work_limits_as_an_empty_proven_context() {
        let mut source = "interface Context { (): void;".to_owned();
        for index in 0..=Checker::CONTRACT_WORK_LIMIT {
            source.push_str(&format!(" property{index}: number;"));
        }
        source.push_str(" } const callback: Context = /*@*/() => 1;");
        with_native_file(&source, |file, marks| {
            let result = project(file, marks[0], true);
            let Value::Record(entries) = &result.items()[0] else { panic!("expected a contract") };
            let (_, context) = entries.iter().find(|(key, _)| key.as_ref() == "contextual_returns")
                .expect("requested context was not represented");
            assert_eq!(*context, Value::Null, "bounded-out context looked like a proven empty return set");
        });
    }

    #[test]
    fn contextual_return_profiles_follow_the_selected_callable_member() {
        with_native_file("const object: {callback: () => void} = /*@*/{callback: () => 1};", |file, marks| {
            assert_eq!(file.syntax.kind(marks[0]), Some(Kind::Object));
            let value = Value::Node(marks[0], file.syntax.text(marks[0]).to_owned().into());
            let result = crate::expr::facts::with_args(vec![
                value.clone(), value,
                Value::list(vec![Value::from("callback")]),
                Value::list(vec![Value::from("contextual_returns")]),
            ], || super::contracts(file, marks[0]));
            let rows = result.items();
            assert_eq!(rows.len(), 1, "selected callable member was lost");
            let profiles = rows[0].key("contextual_returns").items();
            assert_eq!(profiles.len(), 1, "metadata described the object rather than its callable member");
            assert_eq!(profiles[0].key("kind"), Value::from("void"));
        });
    }
}
