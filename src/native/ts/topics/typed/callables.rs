use super::*;
use crate::native::ts::types::Callable;
use std::sync::Arc;

#[cfg(test)]
#[path = "callables/tests.rs"]
mod tests;

struct Contract {
    origin: &'static str,
    sender: Ty,
    receiver: Ty,
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
    let mut projection = Projection { remaining: 4096, complete: !matches!(&arguments, Some(Err(()))), rows: Vec::new() };
    if let Some(Ok(pairs)) = arguments {
        for (sender, receiver) in pairs {
            projection.at_path(file, Contract { origin: "argument", sender, receiver, heritage: None, path: Vec::new() }, &path);
        }
    } else if let Some(pairs) = file.return_types(source) {
        for (sender, receiver) in pairs {
            projection.at_path(file, Contract { origin: "return", sender, receiver, heritage: None, path: Vec::new() }, &path);
        }
    } else {
        let sender = source_type(file, source);
        let (origin, mut receiver) = file.receiver_contract(site, source);
        if matches!(file.syntax.kind(site), Some(Kind::Method | Kind::Prop)) {
            if let Some(member) = file.object_member_contract(site) { receiver = member }
        }
        projection.at_path(file, Contract { origin, sender: sender.clone(), receiver, heritage: None, path: Vec::new() }, &path);
        if file.syntax.is(site, Kind::Declarator) {
            let kind = file.syntax.parent(site).and_then(|owner| file.syntax.word(owner, Field::Kind));
            if matches!(kind, Some("using" | "await using")) {
                let receiver = file.typing().checker.global_named("Disposable", Vec::new());
                projection.at_path(file, Contract { origin: "resource", sender: sender.clone(), receiver, heritage: None, path: Vec::new() }, &path);
            }
        }
        if matches!(file.syntax.kind(site), Some(Kind::Method | Kind::MethodSig | Kind::ClassProp | Kind::PropSig)) {
            for (heritage, receiver) in file.inherited_contracts(site) {
                projection.at_path(file, Contract { origin: "heritage", sender: sender.clone(), receiver, heritage: Some(heritage), path: Vec::new() }, &path);
            }
        }
    }
    Value::list(projection.rows.into_iter().map(|contract| Value::record(vec![
        ("origin".into(), Value::from(contract.origin)),
        ("source".into(), returns(file, &contract.sender)),
        ("target".into(), returns(file, &contract.receiver)),
        ("heritage".into(), contract.heritage.map_or(Value::Null, |node| Value::Node(node, file.syntax.text(node).to_owned().into()))),
        ("member_path".into(), Value::list(contract.path.into_iter().map(Value::from).collect())),
        ("complete".into(), Value::Bool(projection.complete)),
    ])).collect())
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
    for ty in file.callable_return_types(ty) { arms(file, &ty, &mut values) }
    Value::list(values)
}
