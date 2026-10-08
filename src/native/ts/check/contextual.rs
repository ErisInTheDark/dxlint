//! Expected types flow inward through expression containers and transfer sites.

use super::{F, K, NativeFile, Ty};
use super::super::types::{Parameters, Tuple};
use super::super::summary::TupleElement;
use std::{cell::RefCell, collections::HashMap};
#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
#[derive(Default)]
struct PositionWork(Cell<usize>);

type Transfers = HashMap<u32, Vec<(Ty, Ty)>>;

#[derive(Default)]
struct Contexts {
    types: RefCell<HashMap<(u32, bool), Ty>>,
    arrays: RefCell<HashMap<u32, HashMap<u32, Option<usize>>>>,
    calls: RefCell<HashMap<u32, Transfers>>,
    contracts: RefCell<HashMap<u32, Option<Transfers>>>,
}

impl NativeFile<'_> {
    /// The explicit or contextual receiver, before adding inherited contracts.
    pub fn receiver_contract(&self, site: u32, source: u32) -> (&'static str, Ty) {
        let syntax = &self.syntax;
        let explicit = match syntax.kind(site) {
            Some(K::Declarator | K::Param | K::ClassProp | K::As | K::TypeAssertion) =>
                syntax.get(site, F::Type).map(|annotation| ("declared", self.constrained_type_of(annotation))),
            Some(K::Assign | K::AssignPattern) =>
                syntax.get(site, F::Left).map(|left| {
                    let target = self.constrained_type_of(left);
                    ("declared", if target.is_opaque() { self.contextual_constraint_of(source) } else { target })
                }),
            Some(K::Update) =>
                syntax.get(site, F::Arg).map(|target| ("declared", self.constrained_type_of(target))),
            _ => None,
        };
        explicit.unwrap_or_else(|| ("contextual", self.contextual_constraint_of(source)))
    }

    /// Transfer receivers; class properties also retain their inherited contracts.
    pub fn expected_contracts(&self, site: u32, source: u32) -> Vec<(&'static str, Ty)> {
        let mut contracts = vec![self.receiver_contract(site, source)];
        if self.syntax.is(site, K::ClassProp) {
            contracts.extend(self.inherited_contracts(site).into_iter().map(|(_, receiver)| ("heritage", receiver)));
        }
        contracts
    }

    /// An object method or accessor inherits the declared object's member type.
    pub fn object_member_contract(&self, site: u32) -> Option<Ty> {
        let syntax = &self.syntax;
        let object = syntax.parent(site).filter(|&owner| syntax.is(owner, K::Object))?;
        let name = self.property_key_name(site)?;
        let sender = self.constrained_type_of(object);
        let receiver = self.constrained_type(self.contextual_constraint_of(object), object);
        Some(if self.member_keys_match(&sender, &receiver, &name, false) { self.ck().member(&receiver, &name) } else { Ty::Unknown })
    }

    /// A member's contracts from each direct base, retaining their syntax identity.
    pub fn inherited_contracts(&self, site: u32) -> Vec<(u32, Ty)> {
        let syntax = &self.syntax;
        let Some(name) = super::super::statics::property_name(syntax, site) else { return Vec::new() };
        let mut owner = syntax.parent(site);
        if !syntax.is(site, K::ClassProp) && !owner.is_some_and(|owner| matches!(syntax.kind(owner), Some(K::Class | K::Interface))) {
            return Vec::new();
        }
        while owner.is_some_and(|node| !matches!(syntax.kind(node), Some(K::Class | K::Interface))) {
            owner = owner.and_then(|node| syntax.parent(node));
        }
        let Some(owner) = owner else { return Vec::new() };
        syntax.all(owner, F::Extends).chain(syntax.all(owner, F::Implements)).map(|heritage| {
            let target = self.constrained_type_of(heritage);
            let target = if matches!(target, Ty::Class(_)) {
                self.ck().construct(&target, &[], syntax.all(owner, F::TypeArg).map(|argument| self.constrained_type_of(argument)).collect())
            } else { target };
            (heritage, self.ck().member(&target, &name))
        }).collect()
    }

    /// Callback lookup excludes function arguments to avoid inference recursion.
    pub fn contextual_type_of(&self, value: u32) -> Ty {
        self.contextual_projection(value, false)
    }

    /// The same expected type with open parameters projected to their constraints.
    pub fn contextual_constraint_of(&self, value: u32) -> Ty {
        self.contextual_projection(value, true)
    }

    fn contextual_projection(&self, value: u32, bound: bool) -> Ty {
        let contexts = self.cached(|_| Contexts::default());
        if let Some(ty) = contexts.types.borrow().get(&(value, bound)) { return ty.clone() }
        let ty = self.contextual_type(value, bound);
        contexts.types.borrow_mut().insert((value, bound), ty.clone());
        ty
    }

    fn contextual_type(&self, value: u32, bound: bool) -> Ty {
        let s = &self.syntax;
        let mut value = value;
        while let Some(parent) = s.parent(value) {
            let inherits = match s.kind(parent) {
                Some(K::Paren) => true,
                Some(K::Logical) => matches!(s.field(value), Some(F::Left | F::Right)),
                Some(K::Conditional) => matches!(s.field(value), Some(F::Consequent | F::Alternate)),
                Some(K::Sequence) => s.all(parent, F::Expr).last() == Some(value),
                _ => false,
            };
            if !inherits { break }
            value = parent;
        }
        let Some(owner) = s.parent(value) else { return Ty::Unknown };
        if s.is(owner, K::JsxExpression) && s.field(value) == Some(F::Expr) {
            return self.contextual_projection(owner, bound);
        }
        if s.is(owner, K::JsxAttribute) && s.field(value) == Some(F::Value) {
            let Some(opening) = s.parent(owner).filter(|&parent| s.is(parent, K::JsxOpening)) else { return Ty::Unknown };
            let Some(component) = s.get(opening, F::Name) else { return Ty::Unknown };
            let Some(name) = s.get(owner, F::Name).map(|name| s.text(name)) else { return Ty::Unknown };
            let callee = self.type_of(component);
            let props = if matches!(callee, Ty::Class(_)) {
                self.ck().member(&self.ck().construct(&callee, &[], self.type_args(opening)), "props")
            } else {
                self.ck().argument_parameters(&callee, &[], self.type_args(opening), false)
                    .and_then(|(types, _)| types.into_iter().next()).unwrap_or(Ty::Unknown)
            };
            return self.ck().member(&props, name);
        }
        if s.is(owner, K::Prop) && (s.field(value) == Some(F::Value) || s.field(value) == Some(F::Key) && s.get(owner, F::Value).is_none()) {
            if let (Some(object), Some(name)) = (s.parent(owner), super::super::statics::property_name(s, owner)) {
                return self.ck().member(&self.constrained_type(self.contextual_projection(object, bound), object), &name);
            }
        }
        if s.is(owner, K::Array) && s.field(value) == Some(F::Element) {
            return self.array_element_type(owner, value, 0, bound);
        }
        if s.field(value) == Some(F::Init) && matches!(s.kind(owner), Some(K::Declarator | K::ClassProp | K::Param)) {
            return s.get(owner, F::Type).map_or(Ty::Unknown, |annotation| self.expected_type(annotation, bound));
        }
        if s.field(value) == Some(F::Expr) && matches!(s.kind(owner), Some(K::As | K::TypeAssertion | K::Satisfies)) {
            if s.get(owner, F::Type).is_some_and(|annotation| s.text(annotation) == "const") { return self.contextual_projection(owner, bound) }
            return s.get(owner, F::Type).map_or(Ty::Unknown, |annotation| self.expected_type(annotation, bound));
        }
        if s.is(owner, K::AssignPattern) && s.field(value) == Some(F::Right) {
            return s.get(owner, F::Left).map_or_else(|| self.pattern_type(owner, bound), |left| self.pattern_type(left, bound));
        }
        if s.is(owner, K::Assign) && s.field(value) == Some(F::Right) {
            return s.get(owner, F::Left).map_or(Ty::Unknown, |left| self.expected_type(left, bound));
        }
        if let Some((targets, _)) = self.return_contract(value, bound) {
            return Ty::union(targets);
        }
        self.arguments_at(value, true, bound).map_or(Ty::Unknown, |pairs| Ty::union(pairs.into_iter().map(|(_, receiver)| receiver).collect()))
    }

    fn expected_type(&self, node: u32, bound: bool) -> Ty {
        if bound { self.constrained_type_of(node) } else { self.type_of(node) }
    }

    fn array_element_type(&self, array: u32, value: u32, offset: usize, bound: bool) -> Ty {
        let s = &self.syntax;
        let target = self.constrained_type(self.contextual_projection(array, bound), array);
        let contexts = self.cached(|_| Contexts::default());
        if !contexts.arrays.borrow().contains_key(&array) {
            let mut index = Some(0);
            let mut positions = HashMap::new();
            for element in s.all(array, F::Element) {
                #[cfg(test)]
                self.cached(|_| PositionWork::default()).0.update(|work| work + 1);
                positions.insert(element, index);
                if s.is(element, K::Spread) {
                    let source = s.get(element, F::Arg).map_or(Ty::Unknown, |source| self.spread_source_type(source));
                    index = match source {
                        Ty::Tuple(tuple) if !tuple.elements.iter().any(|element| element.optional || element.rest) => index.map(|index| index + tuple.len()),
                        _ => None,
                    };
                } else {
                    index = index.map(|index| index + 1);
                }
            }
            contexts.arrays.borrow_mut().insert(array, positions);
        }
        let position = contexts.arrays.borrow().get(&array).and_then(|positions| positions.get(&value)).copied();
        match position {
            Some(Some(index)) => self.contextual_element(&target, index + offset),
            Some(None) => self.ck().indexed_type(&target, &Ty::Prim("number")),
            None => Ty::Unknown,
        }
    }

    fn pattern_type(&self, node: u32, bound: bool) -> Ty {
        let s = &self.syntax;
        if let Some(annotation) = s.get(node, F::Type) { return self.expected_type(annotation, bound) }
        let Some(owner) = s.parent(node) else { return Ty::Unknown };
        match (s.kind(owner), s.field(node)) {
            (Some(K::Declarator | K::Param), Some(F::Name)) =>
                s.get(owner, F::Type).map_or(Ty::Unknown, |annotation| self.expected_type(annotation, bound)),
            (Some(K::AssignPattern), Some(F::Left)) => self.pattern_type(owner, bound),
            (Some(K::PatternProp), Some(F::Value)) => {
                let Some(pattern) = s.parent(owner) else { return Ty::Unknown };
                super::super::statics::property_name(s, owner).map_or(Ty::Unknown, |name| self.ck().member(&self.pattern_type(pattern, bound), &name))
            }
            (Some(K::ArrayPattern), Some(F::Element)) => {
                let index = s.all(owner, F::Element).position(|element| element == node).unwrap_or(0);
                self.contextual_element(&self.pattern_type(owner, bound), index)
            }
            _ => Ty::Unknown,
        }
    }

    /// A literal spread transfers its iterated values or object properties.
    pub fn spread_types(&self, value: u32) -> Option<Vec<(Ty, Ty)>> {
        let s = &self.syntax;
        if !s.is(value, K::Spread) { return None }
        let owner = s.parent(value)?;
        let source = self.spread_source_type(s.get(value, F::Arg)?);
        let target = self.constrained_type(self.contextual_constraint_of(owner), owner);
        if s.is(owner, K::Object) { return Some(vec![(source, target)]) }
        if !s.is(owner, K::Array) { return None }
        Some(if let Ty::Tuple(tuple) = &source {
            tuple.elements.iter().enumerate().map(|(offset, element)| {
                let sender = if element.rest { self.ck().iteration_element(&element.ty) } else { element.ty.clone() };
                (sender, self.array_element_type(owner, value, offset, true))
            }).collect()
        } else {
            vec![(self.ck().iteration_element(&source), self.array_element_type(owner, value, 0, true))]
        })
    }

    /// A return expression's declared or contextual contract; async values are awaited.
    pub fn return_types(&self, value: u32) -> Option<Vec<(Ty, Ty)>> {
        let (targets, asynchronous) = self.return_contract(value, true)?;
        let sender = self.constrained_type_of(value);
        let sender = if asynchronous { self.ck().awaited(&sender) } else { sender };
        Some(targets.into_iter().map(|receiver| (sender.clone(), receiver)).collect())
    }

    fn return_contract(&self, value: u32, bound: bool) -> Option<(Vec<Ty>, bool)> {
        let s = &self.syntax;
        let parent = s.parent(value)?;
        let mut function = if s.is(parent, K::Return) && s.field(value) == Some(F::Arg) {
            s.parent(parent)?
        } else if s.is(parent, K::Arrow) && s.field(value) == Some(F::Body) && !s.is(value, K::Block) {
            parent
        } else { return None };
        while !matches!(s.kind(function), Some(K::Arrow | K::Function | K::Method)) {
            function = s.parent(function)?;
        }
        let targets = if let Some(annotation) = s.get(function, F::ReturnType) {
            vec![self.expected_type(annotation, bound)]
        } else {
            self.ck().declared_returns(&self.contextual_projection(function, bound))
        };
        let asynchronous = s.has_word(function, F::Modifier, "async");
        let targets = targets.into_iter().map(|receiver| if asynchronous { self.ck().awaited(&receiver) } else { receiver }).collect();
        Some((targets, asynchronous))
    }

    /// Source/parameter pairs at a resolved call position, including tuple spreads.
    pub fn argument_types(&self, value: u32) -> Option<Vec<(Ty, Ty)>> {
        self.arguments_at(value, false, true)
    }

    /// An indexed read's source key and the declared domains that can receive it.
    pub fn index_types(&self, site: u32, source: u32) -> Option<Vec<(Ty, Ty)>> {
        let syntax = &self.syntax;
        if !syntax.is(site, K::Index) || syntax.get(site, F::Index) != Some(source) { return None }
        let object = self.constrained_type_of(syntax.get(site, F::Object)?);
        let sender = self.constrained_type_of(source);
        let receiver = self.ck().index_contract_keys(&object, &sender);
        Some(vec![(sender, receiver)])
    }

    /// All callable parameter contracts at a position, without choosing an overload.
    pub fn argument_contract_types(&self, value: u32) -> Option<Result<Vec<(Ty, Ty)>, ()>> {
        let s = &self.syntax;
        let owner = s.parent(value)?;
        let (call, tagged) = if s.field(value) == Some(F::Arg) && matches!(s.kind(owner), Some(K::Call | K::New)) {
            (owner, false)
        } else if s.is(owner, K::Template) && s.field(value) == Some(F::Expr) {
            let call = s.parent(owner)?;
            if !s.is(call, K::TaggedTemplate) { return None }
            (call, true)
        } else { return None };
        let contexts = self.cached(|_| Contexts::default());
        if let Some(transfers) = contexts.contracts.borrow().get(&call) {
            return Some(transfers.as_ref().map(|pairs| pairs.get(&value).cloned().unwrap_or_default()).ok_or(()));
        }
        let args: Vec<_> = if tagged { s.all(owner, F::Expr).collect() } else { s.all(call, F::Arg).collect() };
        let callee = s.get(call, if tagged { F::Tag } else { F::Callee })?;
        let actuals: Vec<_> = args.iter().map(|&argument| self.type_of(argument)).collect();
        let Some(candidates) = self.ck().argument_contract_parameters(&self.type_of(callee), &actuals, self.type_args(call), s.is(call, K::New)) else {
            contexts.contracts.borrow_mut().insert(call, None);
            return Some(Err(()));
        };
        let mut receivers: HashMap<u32, Vec<(Ty, Vec<Ty>)>> = HashMap::new();
        for (types, rest) in candidates {
            let mut parameters = Parameters::new(types, rest, usize::from(tagged));
            for &argument in &args {
                let pairs = self.argument_transfer(&mut parameters, argument, self.type_of(argument));
                let merged = receivers.entry(argument).or_default();
                for (index, (sender, receiver)) in pairs.into_iter().enumerate() {
                    if let Some((_, receivers)) = merged.get_mut(index) {
                        receivers.push(receiver);
                    } else {
                        merged.push((sender, vec![receiver]));
                    }
                }
            }
        }
        let transfers: Transfers = receivers.into_iter().map(|(argument, pairs)| (
            argument, pairs.into_iter().map(|(sender, receivers)| (sender, Ty::union(receivers))).collect(),
        )).collect();
        let result = transfers.get(&value).cloned().unwrap_or_default();
        contexts.contracts.borrow_mut().insert(call, Some(transfers));
        Some(Ok(result))
    }

    fn arguments_at(&self, value: u32, contextual: bool, bound: bool) -> Option<Vec<(Ty, Ty)>> {
        let s = &self.syntax;
        let owner = s.parent(value)?;
        let (call, tagged) = if s.field(value) == Some(F::Arg) && matches!(s.kind(owner), Some(K::Call | K::New)) {
            (owner, false)
        } else if s.is(owner, K::Template) && s.field(value) == Some(F::Expr) {
            let call = s.parent(owner)?;
            if !s.is(call, K::TaggedTemplate) { return None }
            (call, true)
        } else { return None };
        let contexts = self.cached(|_| Contexts::default());
        if !contextual {
            if let Some(pairs) = contexts.calls.borrow().get(&call) {
                return pairs.get(&value).cloned();
            }
        }
        let args: Vec<_> = if tagged { s.all(owner, F::Expr).collect() } else { s.all(call, F::Arg).collect() };
        let callee = s.get(call, if tagged { F::Tag } else { F::Callee })?;
        let callee = self.type_of(callee);
        let actuals: Vec<_> = args.iter().map(|&argument| {
            if contextual && (argument == value || matches!(s.kind(argument), Some(K::Arrow | K::Function))) {
                Ty::Unknown
            } else { self.type_of(argument) }
        }).collect();
        let type_args = self.type_args(call);
        let (types, rest) = self.ck().argument_context_parameters(&callee, &actuals, type_args, s.is(call, K::New), bound)?;
        let mut parameters = Parameters::new(types, rest, usize::from(tagged));
        let mut transfers = HashMap::new();
        for argument in args {
            let sender = if contextual && argument == value { Ty::Unknown } else { self.type_of(argument) };
            let pairs = self.argument_transfer(&mut parameters, argument, sender);
            if contextual && argument == value { return Some(pairs) }
            transfers.insert(argument, pairs);
        }
        let pairs = transfers.get(&value).cloned();
        if !contextual { contexts.calls.borrow_mut().insert(call, transfers); }
        pairs
    }

    fn argument_transfer(&self, parameters: &mut Parameters, argument: u32, sender: Ty) -> Vec<(Ty, Ty)> {
        let s = &self.syntax;
        let source = if s.is(argument, K::Spread) { s.get(argument, F::Arg).map_or(Ty::Unknown, |source| self.spread_source_type(source)) } else { sender };
        let mut pairs = Vec::new();
        if s.is(argument, K::Spread) {
            if let Ty::Tuple(tuple) = &source {
                for element in &tuple.elements {
                    let sender = if element.rest { self.ck().iteration_element(&element.ty) } else { element.ty.clone() };
                    if let Some(receiver) = parameters.next_recovery() { pairs.push((sender, receiver)); }
                }
                if tuple.elements.iter().any(|element| element.rest) { parameters.consume_remaining(); }
            } else if let Some(receiver) = parameters.next_recovery() { pairs.push((self.ck().iteration_element(&source), receiver)); }
        } else if let Some(receiver) = parameters.next_recovery() { pairs.push((source, receiver)); }
        pairs
    }

    fn spread_source_type(&self, source: u32) -> Ty {
        let s = &self.syntax;
        let source = s.unparen(source);
        if !s.is(source, K::Array) { return self.constrained_type_of(source) }
        Ty::Tuple(Tuple {
            elements: s.all(source, F::Element).map(|element| {
                let rest = s.is(element, K::Spread);
                let ty = if rest { s.get(element, F::Arg).map_or(Ty::Unknown, |argument| self.constrained_type_of(argument)) } else { self.constrained_type_of(element) };
                TupleElement { ty, optional: false, rest }
            }).collect(),
            readonly: false,
        })
    }

    fn contextual_element(&self, ty: &Ty, index: usize) -> Ty {
        match ty {
            Ty::Tuple(tuple) => tuple.at(index),
            Ty::Union(parts) | Ty::Inter(parts) => Ty::union(parts.iter().map(|part| self.contextual_element(part, index)).collect()),
            _ => self.ck().indexed_type(ty, &Ty::Lit(index.to_string().into(), "number")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use super::Ty;

    #[test]
    fn declared_generic_contracts_do_not_replace_inferred_call_results() {
        with_native_file("enum Fruit { Apple } function take<T extends Fruit>(value: T): T { return value; } const result = take(/*@*/1);", |file, nodes| {
            let checker = &file.typing().checker;
            let argument = nodes[0];
            let call = file.syntax.parent(argument).unwrap();
            assert_eq!(checker.equivalent(&file.type_of(call), &Ty::Prim("number")), Some(true));
            let pairs = file.argument_types(argument).unwrap();
            let receiver = &pairs[0].1;
            assert!(matches!(receiver, Ty::Named(symbol, _) if checker.is_enum(symbol)), "{receiver:?}");
            assert_eq!(checker.assignable(&Ty::Prim("number"), receiver), Some(true));
        });
    }

    #[test]
    fn generic_transfer_contracts_keep_explicit_arguments_and_ordered_bounds() {
        for (source, expected) in [
            ("function take<T extends number>(value: T): void {} take<string>(/*@*/'value');", Ty::Prim("string")),
            ("function take<T extends number, V extends T>(value: V): void {} take(/*@*/'value');", Ty::Prim("number")),
            ("class Box<T extends number> { constructor(public value: T) {} } new Box(/*@*/'value');", Ty::Prim("number")),
            ("class Box<T extends number> { constructor(public value: T) {} } new Box<string>(/*@*/'value');", Ty::Prim("string")),
        ] {
            with_native_file(source, |file, nodes| {
                let pairs = file.argument_types(nodes[0]).unwrap();
                assert_eq!(file.typing().checker.equivalent(&pairs[0].1, &expected), Some(true), "{source}: {:?}", pairs[0].1);
            });
        }
    }

    #[test]
    fn querying_every_array_element_walks_positions_once() {
        let source = format!("const values: number[] = [{}];", vec!["/*@*/1"; 64].join(","));
        with_native_file(&source, |file, nodes| {
            for &node in nodes {
                assert!(matches!(file.contextual_type_of(node), Ty::Prim("number")));
            }
            assert!(file.cached(|_| super::PositionWork::default()).0.get() <= 64);
        });
    }

    #[test]
    fn array_literals_contextualise_nested_values_by_position() {
        for (source, expected) in [
            ("const values: string[] = [/*@*/1];", "string"),
            ("const values: [number, string] = [0, /*@*/1];", "string"),
            ("const values: readonly [number, { value: string }] = [0, { value: /*@*/1 }];", "string"),
            ("const values: [number, ...string[]] = [0, 'ok', /*@*/1];", "string"),
            ("declare const pair: [number, boolean]; const values: [number, boolean, string] = [...pair, /*@*/1];", "string"),
        ] {
            with_native_file(source, |file, nodes| {
                assert!(matches!(file.contextual_type_of(nodes[0]), Ty::Prim(kind) if kind == expected), "{source}");
            });
        }
    }
}
