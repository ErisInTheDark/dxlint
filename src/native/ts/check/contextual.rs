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
}

impl NativeFile<'_> {
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
            let Some(element) = s.parent(owner).filter(|&parent| s.is(parent, K::JsxElement)) else { return Ty::Unknown };
            let Some(component) = s.get(element, F::Name) else { return Ty::Unknown };
            let Some(name) = s.get(owner, F::Name).map(|name| s.text(name)) else { return Ty::Unknown };
            let callee = self.type_of(component);
            let props = if matches!(callee, Ty::Class(_)) {
                self.ck().member(&self.ck().construct(&callee, &[], self.type_args(element)), "props")
            } else {
                self.ck().argument_parameters(&callee, &[], self.type_args(element), false)
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
    pub fn argument_types(&self, value: u32, contextual: bool) -> Option<Vec<(Ty, Ty)>> {
        self.arguments_at(value, contextual, true)
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
        let (types, rest) = if contextual {
            let (sig, env) = self.ck().signature_for(&callee, &actuals, s.is(call, K::New))?;
            let mut env = (*env).clone();
            for parameter in &sig.type_params {
                let ty = if bound {
                    parameter.constraint.as_ref().map_or_else(|| Ty::Param(parameter.name.clone()), |bound| self.ck().eval(bound, &env))
                } else { Ty::Param(parameter.name.clone()) };
                env.tparams.push((parameter.name.clone(), ty));
            }
            (sig.params.iter().map(|parameter| parameter.ty.as_ref().map_or(Ty::Unknown, |ty| self.ck().eval(ty, &env))).collect(), sig.params.last().is_some_and(|parameter| parameter.rest))
        } else {
            self.ck().argument_parameters(&callee, &actuals, type_args, s.is(call, K::New))?
        };
        let mut parameters = Parameters::new(types, rest, usize::from(tagged));
        let mut transfers = HashMap::new();
        for argument in args {
            let sender = if contextual && argument == value { Ty::Unknown } else { self.type_of(argument) };
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
            if contextual && argument == value { return Some(pairs) }
            transfers.insert(argument, pairs);
        }
        let pairs = transfers.get(&value).cloned();
        if !contextual { contexts.calls.borrow_mut().insert(call, transfers); }
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
