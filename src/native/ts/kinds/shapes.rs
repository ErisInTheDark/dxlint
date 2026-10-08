//! Parser field targets, before pattern layers are grafted into the tree.

use super::{Field, Kind, KINDS};
use Kind as K;

// These groups follow parser return paths, not TypeScript's semantic validity
// checks. For example, assignment left sides are parsed by conditional().
const PRIMARY: &[Kind] = &[
    K::Ident, K::PrivateName, K::This, K::Super, K::Null, K::Bool, K::Number, K::BigInt, K::String, K::Regex,
    K::Template, K::Paren, K::Array, K::Object, K::Function, K::Class, K::JsxElement, K::JsxFragment, K::MetaProperty,
    K::ImportCall,
];

const fn extend<const N: usize>(base: &[Kind], extra: &[Kind]) -> [Kind; N] {
    let mut out = [K::Ident; N];
    let mut at = 0;
    while at < N {
        out[at] = if at < base.len() { base[at] } else { extra[at - base.len()] };
        at += 1;
    }
    out
}

macro_rules! group {
    ($base:ident; $($kind:ident),* $(,)?) => {
        &extend::<{ $base.len() + [$(K::$kind),*].len() }>($base, &[$(K::$kind),*])
    };
}

const LHS: &[Kind] = group!(PRIMARY; New, Member, Index, Call, TaggedTemplate, NonNull, Instantiation);
const NEW_CALLEE: &[Kind] = group!(PRIMARY; New, Member, Index, TaggedTemplate, NonNull);
const UNARY: &[Kind] = group!(LHS; Unary, Update, Await, TypeAssertion);
const VALUE_BINARY: &[Kind] = group!(UNARY; Binary, As, Satisfies);
const BINARY: &[Kind] = group!(VALUE_BINARY; Logical);
const CONDITIONAL: &[Kind] = group!(BINARY; Conditional);
const ASSIGNMENT: &[Kind] = group!(CONDITIONAL; Assign, Arrow, Yield);
const EXPRESSION: &[Kind] = group!(ASSIGNMENT; Sequence);
const ARGUMENT: &[Kind] = group!(ASSIGNMENT; Spread);
const ARRAY_ELEMENT: &[Kind] = group!(ARGUMENT; Hole);
const LOOP_INIT: &[Kind] = group!(EXPRESSION; Var);
const ARROW_BODY: &[Kind] = group!(ASSIGNMENT; Block);
const PROPERTY_VALUE: &[Kind] = group!(ASSIGNMENT; AssignPattern);
const JSX_EXPRESSION: &[Kind] = group!(EXPRESSION; Spread);

const STATEMENT: &[Kind] = &[
    K::Block, K::ExprStmt, K::Var, K::If, K::For, K::ForIn, K::ForOf, K::While, K::DoWhile, K::Return, K::Throw,
    K::Break, K::Continue, K::Switch, K::Try, K::Labeled, K::Empty, K::Debugger, K::With, K::Function, K::Class,
    K::Import, K::ImportEquals, K::Export, K::ExportAssign, K::Interface, K::TypeAlias, K::Enum, K::Namespace,
];
const DECLARATION: &[Kind] = &[
    K::Var, K::Function, K::Class, K::Interface, K::TypeAlias, K::Enum, K::Namespace, K::Import, K::ImportEquals, K::Export,
];
const BINDING: &[Kind] = &[K::Ident, K::ObjectPattern, K::ArrayPattern];
const BINDING_ELEMENT: &[Kind] = group!(BINDING; AssignPattern);
const PATTERN_ELEMENT: &[Kind] = group!(BINDING_ELEMENT; Rest, Hole);
const PARAM_NAME: &[Kind] = group!(BINDING; Rest, This);
const PROPERTY_KEY: &[Kind] = &[K::Ident, K::PrivateName, K::String, K::Number, K::BigInt, K::Computed];
const TYPE_MEMBER: &[Kind] = &[K::PropSig, K::MethodSig, K::CallSig, K::IndexSig];
const CLASS_MEMBER: &[Kind] = &[K::ClassProp, K::Method, K::StaticBlock, K::IndexSig];

const PRIMARY_TYPE: &[Kind] = &[
    K::ParenType, K::Mapped, K::TypeLit, K::TupleType, K::LiteralType, K::TemplateType, K::TypeQuery, K::ImportType,
    K::TypeKeyword, K::TypeRef,
];
const OPERATOR_TYPE: &[Kind] = group!(PRIMARY_TYPE; TypeOperator, Infer, ArrayType, IndexedAccess);
const INTERSECTION_TYPE: &[Kind] = group!(OPERATOR_TYPE; Intersection);
const UNION_TYPE: &[Kind] = group!(INTERSECTION_TYPE; Union);
const NON_CONDITIONAL_TYPE: &[Kind] = group!(UNION_TYPE; FnType);
const TYPE: &[Kind] = group!(NON_CONDITIONAL_TYPE; ConditionalType);
const RETURN_TYPE: &[Kind] = group!(TYPE; TypePredicate);

const TOKENS: &[Kind] = &[
    K::Comma, K::Semicolon, K::Colon, K::Dot, K::Question, K::Ellipsis, K::Equals, K::OpenParen, K::CloseParen,
    K::OpenBracket, K::CloseBracket, K::OpenBrace, K::CloseBrace, K::OpenAngle, K::CloseAngle, K::Keyword, K::Punctuator,
];

const EMPTY: FieldShape = FieldShape { kinds: &[], text: false, any: false };
const TEXT: FieldShape = FieldShape { text: true, ..EMPTY };

/// Native targets and plain labelled text are separate domains. `any`
/// explicitly marks an unresolved parser field, not an absent field.
#[derive(Clone, Copy, Debug)]
pub struct FieldShape {
    pub kinds: &'static [Kind],
    pub text: bool,
    pub any: bool,
}

pub fn field_shape(kind: Kind, field: Field) -> FieldShape {
    use Field as F;

    // Token attachment is a separate owner, independent of selected kinds.
    if field == F::Token {
        return if kind.is_token() { EMPTY } else { FieldShape { kinds: TOKENS, ..EMPTY } };
    }
    if !KINDS[kind as usize].1.contains(&field) {
        return EMPTY;
    }
    let kinds = match (kind, field) {
        // token_text()/text_node() create TEXT, never ts_keyword/punctuator.
        // ImportEquals.Modifier is advertised but import() never attaches it.
        (K::ImportEquals, F::Modifier) => &[],
        (_, F::Modifier | F::Optional | F::Op | F::Kind) => return TEXT,
        (K::Template | K::TemplateType, F::Quasi) => return TEXT,
        (K::Program | K::Block | K::Case, F::Body)
        | (K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::Labeled | K::With, F::Body)
        | (K::If, F::Consequent | F::Alternate) => STATEMENT,
        (K::ExprStmt | K::ExportAssign | K::Paren, F::Expr)
        | (K::If | K::For | K::While | K::DoWhile | K::Case, F::Test)
        | (K::For, F::Update)
        | (K::ForIn, F::Right)
        | (K::Return | K::Throw, F::Arg)
        | (K::Switch, F::Discriminant)
        | (K::With, F::Object)
        | (K::Index, F::Index) => EXPRESSION,
        (K::For, F::Init) | (K::ForIn | K::ForOf, F::Left) => LOOP_INIT,
        (K::ForOf, F::Right)
        | (K::Declarator | K::ClassProp | K::EnumMember | K::Param, F::Init)
        | (K::Export, F::Expr)
        | (K::Spread | K::Yield | K::JsxSpreadAttribute, F::Arg)
        | (K::Computed, F::Expr)
        | (K::Template | K::Sequence, F::Expr)
        | (K::Assign | K::AssignPattern, F::Right)
        | (K::Conditional, F::Consequent | F::Alternate) => ASSIGNMENT,
        (K::Break | K::Continue | K::Labeled, F::Label)
        | (K::Function | K::Class | K::Interface | K::TypeAlias | K::Enum | K::ImportEquals | K::TypeParam | K::TupleMember | K::Rest, F::Name)
        | (K::Import, F::DefaultImport | F::Namespace)
        | (K::ImportSpec, F::Local) => &[K::Ident],
        (K::Declarator, F::Name) | (K::Try, F::Param) | (K::Rest, F::Arg) | (K::AssignPattern, F::Left) => BINDING,
        (K::Param, F::Name) => PARAM_NAME,
        (K::Var, F::Decl) => &[K::Declarator],
        (K::Switch, F::Case) => &[K::Case],
        (K::Try, F::Block | F::Handler | F::Finalizer)
        | (K::Function | K::Method | K::StaticBlock | K::Namespace, F::Body) => &[K::Block],
        (K::Arrow, F::Body) => ARROW_BODY,
        (_, F::TypeParam) => &[K::TypeParam],
        (_, F::ReturnType) => RETURN_TYPE,
        (_, F::TypeArg | F::Constraint | F::Default | F::Implements)
        | (K::Declarator | K::Try | K::Param | K::ClassProp | K::IndexSig | K::TypeAlias | K::Rest | K::TupleMember | K::PropSig
            | K::Mapped | K::ParenType | K::TemplateType | K::TypePredicate, F::Type)
        | (K::As | K::Satisfies | K::TypeAssertion, F::Type)
        | (K::IndexedAccess, F::Index)
        | (K::ConditionalType, F::Consequent | F::Alternate)
        | (K::Interface, F::Extends)
        | (K::ImportType, F::Arg)
        | (K::Mapped, F::Name) => TYPE,
        (K::Union, F::Type) => INTERSECTION_TYPE,
        (K::Intersection, F::Type) | (K::TypeOperator | K::ArrayType, F::Type)
        | (K::IndexedAccess, F::Object) => OPERATOR_TYPE,
        (K::ConditionalType, F::Check) => UNION_TYPE,
        (K::ConditionalType, F::Extends) => NON_CONDITIONAL_TYPE,
        (K::Function | K::Arrow | K::Method | K::FnType | K::MethodSig | K::CallSig | K::IndexSig, F::Param) => &[K::Param],
        (_, F::Decorator) => &[K::Decorator],
        (K::Decorator, F::Expr) | (K::Class, F::Extends) | (K::ImportEquals, F::Value)
        | (K::Member | K::Index, F::Object) | (K::Call, F::Callee)
        | (K::TaggedTemplate, F::Tag) | (K::NonNull | K::Instantiation, F::Expr) => LHS,
        (K::New, F::Callee) => NEW_CALLEE,
        (K::Class, F::Member) => CLASS_MEMBER,
        (K::Interface | K::TypeLit, F::Member) => TYPE_MEMBER,
        (K::Enum, F::Member) => &[K::EnumMember],
        (K::Method | K::ClassProp | K::PatternProp | K::Prop | K::PropSig | K::MethodSig, F::Key)
        | (K::EnumMember, F::Name) => PROPERTY_KEY,
        (K::Import | K::Export, F::From) => &[K::String],
        (K::Import, F::Specifier) => &[K::ImportSpec],
        (K::Export, F::Specifier) => &[K::ExportSpec],
        (K::ImportSpec, F::Imported) | (K::ExportSpec, F::Local | F::Exported) | (K::Export, F::Namespace) => &[K::Ident, K::String],
        (K::Export, F::Decl) => DECLARATION,
        (K::Namespace, F::Name) => &[K::Ident, K::String, K::QualifiedName],
        (K::QualifiedName, F::Left) => &[K::Ident, K::String, K::This, K::QualifiedName],
        (K::QualifiedName, F::Right) => &[K::Ident, K::PrivateName],
        (K::ObjectPattern, F::Property) => &[K::PatternProp],
        (K::ObjectPattern, F::Rest) => &[K::Rest],
        (K::ArrayPattern, F::Element) => PATTERN_ELEMENT,
        (K::PatternProp, F::Value) => BINDING_ELEMENT,
        (K::Prop, F::Value) => PROPERTY_VALUE,
        (K::Array, F::Element) => ARRAY_ELEMENT,
        (K::Object, F::Property) => &[K::Prop, K::Method, K::Spread],
        (K::Member, F::Property) => &[K::Ident, K::PrivateName],
        (K::Call | K::New | K::ImportCall, F::Arg) => ARGUMENT,
        (K::Unary | K::Update | K::Await, F::Arg) | (K::TypeAssertion, F::Expr) => UNARY,
        (K::Binary, F::Left | F::Right) | (K::As | K::Satisfies, F::Expr) => VALUE_BINARY,
        (K::Logical, F::Left | F::Right) | (K::Conditional, F::Test) => BINARY,
        (K::Assign, F::Left) => CONDITIONAL,
        (K::TaggedTemplate, F::Quasi) => &[K::Template],
        (K::JsxElement | K::JsxFragment, F::Opening) => &[K::JsxOpening],
        (K::JsxElement | K::JsxFragment, F::Closing) => &[K::JsxClosing],
        (K::JsxElement | K::JsxFragment, F::Body) => &[K::JsxText, K::JsxExpression, K::JsxElement, K::JsxFragment],
        (K::JsxOpening, F::Name) => &[K::Ident, K::This, K::Member, K::JsxName],
        (K::JsxOpening, F::Property) => &[K::JsxAttribute, K::JsxSpreadAttribute],
        (K::JsxClosing | K::JsxAttribute, F::Name) => &[K::JsxName],
        (K::JsxAttribute, F::Value) => &[K::String, K::JsxElement, K::JsxFragment, K::JsxExpression],
        (K::JsxExpression, F::Expr) => JSX_EXPRESSION,
        (K::TypeRef, F::Name) => &[K::Ident, K::QualifiedName],
        (K::ImportType, F::Qualifier) => &[K::Ident, K::This, K::QualifiedName],
        (K::TypeQuery, F::Name) => &[K::Ident, K::This, K::QualifiedName, K::ImportType],
        (K::TupleType, F::Element) => &[K::TupleMember, K::Rest],
        (K::TypePredicate, F::Name) => &[K::Ident, K::This],
        (K::LiteralType, F::Value) => &[K::String, K::Number, K::BigInt, K::Bool],
        // The registry also names ObjectPattern.Type/ArrayPattern.Type,
        // currently never attached: parameter/declarator owns annotations.
        _ => &[],
    };
    FieldShape { kinds, ..EMPTY }
}

/// Native child kinds only; use `field_shape` to also check labelled text.
pub fn field_kinds(kind: Kind, field: Field) -> &'static [Kind] {
    field_shape(kind, field).kinds
}

#[cfg(test)]
pub(crate) fn validate_shapes(parsed: &crate::native::ts::Parsed) {
    use super::{ALL_FIELDS, ALL_KINDS, KINDS};
    use crate::grammar::TEXT;

    let mut pending = vec![parsed.root];
    while let Some(id) = pending.pop() {
        let node = &parsed.nodes[id as usize];
        if node.matcher == TEXT {
            assert!(node.children.is_empty(), "labelled text cannot own syntax");
            continue;
        }
        let kind = ALL_KINDS[node.matcher as usize];
        for edge in &node.children {
            let field = ALL_FIELDS[edge.label as usize];
            assert!(field == Field::Token || KINDS[kind as usize].1.contains(&field), "{kind:?}.{field:?} missing from kind registry");
            let child = &parsed.nodes[edge.node as usize];
            let shape = field_shape(kind, field);
            if child.matcher == TEXT {
                assert!(shape.any || shape.text, "{kind:?}.{field:?} does not allow labelled text");
            } else {
                let target = ALL_KINDS[child.matcher as usize];
                assert!(shape.any || shape.kinds.contains(&target), "{kind:?}.{field:?} does not allow {target:?}");
            }
            pending.push(edge.node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::ts::{parse, tokens};

    #[test]
    fn parser_edges_follow_field_shapes() {
        let fixtures = [
            ("try { a(); } catch ({message}: unknown) { throw message; } finally {}", false),
            ("for await (const x of xs) {} for (a, b in obj) {} for (let x=0; x<1; x++) {}", false),
            ("const {x:y=1, ...rest}=obj; const [a,,b=2,...tail]=xs; const o={x=1, get y(){return 1}, ...rest};", false),
            ("@dec export default class C<T> extends mixin(Base)<T> implements I<T> { @dec accessor x?: T; static {} constructor(@dec public x: T) {} *g(){yield* xs} [key?: string]: T; }", false),
            ("type F = abstract new <T>(x: T) => T; interface I { readonly x: number; readonly f<T>(x:T): T; get y(): number; new(): I; (): void; [key?: string]: number }", false),
            ("type T = [head?: string, ...tail: number[]]; type M<T> = {-readonly [K in keyof T as K]-?: T[K]};", false),
            ("type X<T> = T extends infer U extends string ? U : never; type Q = typeof this.#x; type V = typeof import('m').X; type L = -1n | true | `a${string}`;", false),
            ("declare module 'a'; namespace A.B { export import C = D.E; } import type X = require('x'); export type * as 'n' from 'm'; export {'a' as 'b'} from 'm'; export = a,b;", false),
            ("const a = <this.Foo<T> flag ns:x='a' child=<div/> {...p}>{...xs}{a,b}<></></this.Foo>;", true),
            ("function f(this: C, ...args: T[]): asserts this is C {} const g = async <T,>(x: T): T => x;", true),
        ];
        for (source, jsx) in fixtures {
            let mut parsed = parse(source, jsx).unwrap_or_else(|error| panic!("{error:?}: {source}"));
            validate_shapes(&parsed);
            tokens::attach_selected(source, &mut parsed, &tokens::ALL_TOKENS);
            validate_shapes(&parsed);
        }
    }

    /// Optional external fixtures keep licensed projects out of this repo.
    #[test]
    #[ignore = "requires ZLINT_TS_SHAPE_FIXTURES external JS/TS paths"]
    fn project_fixture_edges_follow_field_shapes() {
        let paths = std::env::var_os("ZLINT_TS_SHAPE_FIXTURES").expect("ZLINT_TS_SHAPE_FIXTURES");
        for path in std::env::split_paths(&paths) {
            let source = crate::native::ts::reads::read(&path).expect("shape fixture");
            let jsx = crate::native::ts::reads(&path).expect("JS/TS fixture");
            let mut parsed = parse(&source, jsx).unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
            validate_shapes(&parsed);
            tokens::attach_selected(&source, &mut parsed, &tokens::ALL_TOKENS);
            validate_shapes(&parsed);
            println!("{}: {} bytes, {} nodes checked", path.display(), source.len(), parsed.nodes.len());
        }
    }
}
