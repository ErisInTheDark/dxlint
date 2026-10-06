//! Logical ESTree syntax, including nodes absent from the native grammar.

use crate::selector::{Attribute, Selector, Tree};
use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;
use std::borrow::Cow;

struct Descriptor {
    name: &'static str,
    spacing: bool,
    fields: &'static [(F, &'static str, bool)],
}

macro_rules! descriptors {
    ($($kind:ident => $name:literal, $spacing:literal, [$($field:ident => $key:literal : $array:literal),*];)*) => {
        fn descriptor(kind: K) -> Option<Descriptor> {
            Some(match kind {
                $(K::$kind => Descriptor { name: $name, spacing: $spacing, fields: &[$((F::$field, $key, $array)),*] },)*
                _ => return None,
            })
        }
    };
}

descriptors! {
    Program => "Program", true, [Body => "body": true];
    Block => "BlockStatement", true, [Body => "body": true];
    ExprStmt => "ExpressionStatement", true, [Expr => "expression": false];
    Var => "VariableDeclaration", true, [Decl => "declarations": true];
    Declarator => "VariableDeclarator", true, [Name => "id": false, Init => "init": false];
    If => "IfStatement", false, [Test => "test": false, Consequent => "consequent": false, Alternate => "alternate": false];
    For => "ForStatement", false, [Init => "init": false, Test => "test": false, Update => "update": false, Body => "body": false];
    ForIn => "ForInStatement", false, [Left => "left": false, Right => "right": false, Body => "body": false];
    ForOf => "ForOfStatement", false, [Left => "left": false, Right => "right": false, Body => "body": false];
    While => "WhileStatement", false, [Test => "test": false, Body => "body": false];
    DoWhile => "DoWhileStatement", false, [Body => "body": false, Test => "test": false];
    Return => "ReturnStatement", true, [Arg => "argument": false];
    Throw => "ThrowStatement", false, [Arg => "argument": false];
    Break => "BreakStatement", false, [Label => "label": false];
    Continue => "ContinueStatement", false, [Label => "label": false];
    Switch => "SwitchStatement", false, [Discriminant => "discriminant": false, Case => "cases": true];
    Case => "SwitchCase", false, [Test => "test": false, Body => "consequent": true];
    Try => "TryStatement", false, [Block => "block": false, Finalizer => "finalizer": false];
    Labeled => "LabeledStatement", false, [Label => "label": false, Body => "body": false];
    Empty => "EmptyStatement", false, [];
    Debugger => "DebuggerStatement", false, [];
    With => "WithStatement", false, [Object => "object": false, Body => "body": false];
    Function => "FunctionDeclaration", false, [Name => "id": false, Param => "params": true, Body => "body": false, Decorator => "decorators": true];
    Arrow => "ArrowFunctionExpression", false, [Param => "params": true, Body => "body": false];
    Class => "ClassDeclaration", false, [Name => "id": false, Extends => "superClass": false, Implements => "implements": true, Decorator => "decorators": true];
    Method => "MethodDefinition", true, [Key => "key": false, Decorator => "decorators": true];
    ClassProp => "PropertyDefinition", true, [Key => "key": false, Init => "value": false, Decorator => "decorators": true];
    StaticBlock => "StaticBlock", false, [Body => "body": true];
    IndexSig => "TSIndexSignature", false, [Param => "parameters": true];
    Decorator => "Decorator", false, [Expr => "expression": false];
    Import => "ImportDeclaration", true, [Specifier => "specifiers": true, From => "source": false];
    ImportSpec => "ImportSpecifier", true, [Imported => "imported": false, Local => "local": false];
    ImportEquals => "TSImportEqualsDeclaration", false, [Name => "id": false, Value => "moduleReference": false];
    Export => "ExportNamedDeclaration", false, [Decl => "declaration": false, Specifier => "specifiers": true, From => "source": false];
    ExportSpec => "ExportSpecifier", false, [Local => "local": false, Exported => "exported": false];
    ExportAssign => "TSExportAssignment", false, [Expr => "expression": false];
    Interface => "TSInterfaceDeclaration", true, [Name => "id": false, Extends => "extends": true];
    TypeAlias => "TSTypeAliasDeclaration", true, [Name => "id": false, Type => "typeAnnotation": false];
    Enum => "TSEnumDeclaration", false, [Name => "id": false];
    EnumMember => "TSEnumMember", true, [Name => "id": false, Init => "initializer": false];
    Namespace => "TSModuleDeclaration", false, [Name => "id": false, Body => "body": false];
    ObjectPattern => "ObjectPattern", true, [Property => "properties": true, Rest => "properties": true];
    ArrayPattern => "ArrayPattern", true, [Element => "elements": true];
    AssignPattern => "AssignmentPattern", true, [Left => "left": false, Right => "right": false];
    PatternProp => "Property", true, [Key => "key": false, Value => "value": false];
    Rest => "RestElement", false, [Arg => "argument": false];
    Ident => "Identifier", false, [];
    PrivateName => "PrivateIdentifier", false, [];
    This => "ThisExpression", false, [];
    Super => "Super", false, [];
    Null => "Literal", false, [];
    Bool => "Literal", false, [];
    Number => "Literal", false, [];
    BigInt => "Literal", false, [];
    String => "Literal", false, [];
    Regex => "Literal", false, [];
    Template => "TemplateLiteral", false, [Expr => "expressions": true];
    TaggedTemplate => "TaggedTemplateExpression", false, [Tag => "tag": false, Quasi => "quasi": false];
    Array => "ArrayExpression", true, [Element => "elements": true];
    Object => "ObjectExpression", true, [Property => "properties": true];
    Prop => "Property", true, [Key => "key": false, Value => "value": false];
    Spread => "SpreadElement", false, [Arg => "argument": false];
    Member => "MemberExpression", false, [Object => "object": false, Property => "property": false];
    Index => "MemberExpression", false, [Object => "object": false, Index => "property": false];
    Call => "CallExpression", true, [Callee => "callee": false, Arg => "arguments": true];
    New => "NewExpression", true, [Callee => "callee": false, Arg => "arguments": true];
    Unary => "UnaryExpression", false, [Arg => "argument": false];
    Update => "UpdateExpression", false, [Arg => "argument": false];
    Await => "AwaitExpression", false, [Arg => "argument": false];
    Yield => "YieldExpression", false, [Arg => "argument": false];
    Binary => "BinaryExpression", true, [Left => "left": false, Right => "right": false];
    Logical => "LogicalExpression", true, [Left => "left": false, Right => "right": false];
    Assign => "AssignmentExpression", true, [Left => "left": false, Right => "right": false];
    Conditional => "ConditionalExpression", true, [Test => "test": false, Consequent => "consequent": false, Alternate => "alternate": false];
    Sequence => "SequenceExpression", false, [Expr => "expressions": true];
    As => "TSAsExpression", false, [Expr => "expression": false, Type => "typeAnnotation": false];
    Satisfies => "TSSatisfiesExpression", false, [Expr => "expression": false, Type => "typeAnnotation": false];
    NonNull => "TSNonNullExpression", false, [Expr => "expression": false];
    TypeAssertion => "TSTypeAssertion", false, [Type => "typeAnnotation": false, Expr => "expression": false];
    MetaProperty => "MetaProperty", false, [];
    ImportCall => "ImportExpression", false, [Arg => "source": false];
    Instantiation => "TSInstantiationExpression", false, [Expr => "expression": false];
    JsxElement => "JSXElement", false, [Body => "children": true];
    JsxFragment => "JSXFragment", false, [Body => "children": true];
    JsxAttribute => "JSXAttribute", false, [Name => "name": false, Value => "value": false];
    JsxSpreadAttribute => "JSXSpreadAttribute", false, [Arg => "argument": false];
    JsxExpression => "JSXExpressionContainer", false, [Expr => "expression": false];
    JsxText => "JSXText", false, [];
    JsxName => "JSXIdentifier", false, [];
    TypeRef => "TSTypeReference", false, [Name => "typeName": false];
    QualifiedName => "TSQualifiedName", false, [Left => "left": false, Right => "right": false];
    TypeParam => "TSTypeParameter", false, [Name => "name": false, Constraint => "constraint": false, Default => "default": false];
    TypeKeyword => "TSKeyword", false, [];
    Union => "TSUnionType", false, [Type => "types": true];
    Intersection => "TSIntersectionType", false, [Type => "types": true];
    ArrayType => "TSArrayType", false, [Type => "elementType": false];
    TupleType => "TSTupleType", false, [Element => "elementTypes": true];
    TupleMember => "TSNamedTupleMember", false, [Name => "label": false, Type => "elementType": false];
    FnType => "TSFunctionType", false, [Param => "params": true];
    TypeLit => "TSTypeLiteral", true, [Member => "members": true];
    PropSig => "TSPropertySignature", true, [Key => "key": false];
    MethodSig => "TSMethodSignature", false, [Key => "key": false, Param => "params": true];
    CallSig => "TSCallSignatureDeclaration", false, [Param => "params": true];
    TypeQuery => "TSTypeQuery", false, [Name => "exprName": false];
    TypeOperator => "TSTypeOperator", false, [Type => "typeAnnotation": false];
    IndexedAccess => "TSIndexedAccessType", false, [Object => "objectType": false, Index => "indexType": false];
    ConditionalType => "TSConditionalType", false, [Check => "checkType": false, Extends => "extendsType": false, Consequent => "trueType": false, Alternate => "falseType": false];
    Infer => "TSInferType", false, [TypeParam => "typeParameter": false];
    Mapped => "TSMappedType", false, [Name => "nameType": false, Type => "typeAnnotation": false];
    TemplateType => "TSTemplateLiteralType", false, [Type => "types": true];
    TypePredicate => "TSTypePredicate", false, [Name => "parameterName": false];
    ImportType => "TSImportType", false, [Arg => "argument": false, Qualifier => "qualifier": false];
    LiteralType => "TSLiteralType", false, [Value => "literal": false];
}

pub(super) fn spacing_owner(kind: K) -> &'static str {
    descriptor(kind).filter(|d| d.spacing).map_or("", |d| d.name)
}

enum Property {
    Null, Bool(bool), Number(f64), BigInt(String), ObjectText(String), Text(String), Node(u32), Array(Vec<Option<u32>>),
    Record(Vec<(&'static str, Property)>),
}

struct Node {
    origin: u32,
    kind: &'static str,
    start: u32,
    end: u32,
    parent: Option<u32>,
    field: &'static str,
    position: Option<(usize, usize)>,
    children: Vec<u32>,
    properties: Vec<(&'static str, Property)>,
}

pub(super) struct Projection {
    nodes: Vec<Node>,
    native: Vec<Vec<u32>>,
}

impl Projection {
    pub(super) fn new(file: &NativeFile<'_>) -> Self {
        let mut projection = Self { nodes: vec![], native: vec![vec![]; file.syntax.tree.nodes.len()] };
        let lexed = file.cached(super::tokens::lex);
        Builder { syntax: &file.syntax, tokens: &lexed.0, projection: &mut projection }.project(file.syntax.root(), None, "", None, false);
        projection
    }
    pub(super) fn nodes(&self) -> std::ops::Range<u32> { 0..self.nodes.len() as u32 }
    fn property(&self, node: u32, key: &str) -> Option<&Property> {
        self.nodes[node as usize].properties.iter().find(|(name, _)| *name == key).map(|(_, value)| value)
    }
    fn add(&mut self, syntax: &Syntax<'_>, origin: u32, kind: &'static str, parent: Option<u32>, field: &'static str, position: Option<(usize, usize)>) -> u32 {
        let source = syntax.tree.node(origin);
        let id = self.nodes.len() as u32;
        self.nodes.push(Node { origin, kind, start: source.start, end: source.end, parent, field, position, children: vec![], properties: vec![] });
        self.native[origin as usize].push(id);
        if let Some(parent) = parent { self.nodes[parent as usize].children.push(id); }
        id
    }
    fn set(&mut self, node: u32, key: &'static str, value: Property) {
        let properties = &mut self.nodes[node as usize].properties;
        if let Some((_, existing)) = properties.iter_mut().find(|(name, _)| *name == key) { *existing = value; } else { properties.push((key, value)); }
    }
    pub(super) fn matches(&self, native: u32, selector: &Selector) -> Result<bool, String> {
        if let Some(nodes) = self.native.get(native as usize) {
            for &node in nodes { if selector.matches(self, node)? { return Ok(true); } }
        }
        Ok(false)
    }
    pub(super) fn spans(&self, native: u32, selector: &Selector) -> Result<Vec<crate::selector::Span>, String> {
        let Some(roots) = self.native.get(native as usize) else { return Ok(vec![]) };
        let mut included = vec![false; self.nodes.len()];
        let mut pending = roots.clone();
        while let Some(node) = pending.pop() {
            if std::mem::replace(&mut included[node as usize], true) { continue; }
            pending.extend(self.children(node));
        }
        let mut spans = vec![];
        for node in self.nodes() {
            if included[node as usize] && selector.matches(self, node)? {
                let n = &self.nodes[node as usize];
                spans.push(crate::selector::Span { node: n.origin, kind: n.kind, start: n.start, end: n.end });
            }
        }
        Ok(spans)
    }
}

impl Tree for Projection {
    fn kind(&self, node: u32) -> &str { self.nodes[node as usize].kind }
    fn parent(&self, node: u32) -> Option<u32> { self.nodes[node as usize].parent }
    fn children(&self, node: u32) -> &[u32] { &self.nodes[node as usize].children }
    fn field(&self, node: u32) -> &str { self.nodes[node as usize].field }
    fn array_position(&self, node: u32) -> Option<(usize, usize)> { self.nodes[node as usize].position }
    fn attribute(&self, mut node: u32, path: &[String]) -> Attribute<'_> {
        let mut value: Option<&Property> = None;
        for (i, key) in path.iter().enumerate() {
            if key == "type" && value.is_none() {
                return if i + 1 == path.len() { Attribute::String(Cow::Borrowed(self.kind(node))) } else { Attribute::Missing };
            }
            value = match value {
                None => self.property(node, key),
                Some(Property::Node(next)) => { node = *next; if key == "type" && i + 1 == path.len() { return Attribute::String(self.kind(node).into()); } self.property(node, key) }
                Some(Property::Record(properties)) => properties.iter().find(|(name, _)| *name == key).map(|(_, value)| value),
                Some(Property::Array(nodes)) => {
                    if key == "length" && i + 1 == path.len() { return Attribute::Number(nodes.len() as f64); }
                    let Some(slot) = key.parse::<usize>().ok().and_then(|index| nodes.get(index)) else { return Attribute::Missing };
                    let Some(index) = *slot else { return if i + 1 == path.len() { Attribute::Null } else { Attribute::Missing } };
                    if i + 1 == path.len() { return Attribute::Object("[object Object]".into()); }
                    node = index;
                    None
                }
                _ => return Attribute::Missing,
            };
            if value.is_none() && i + 1 == path.len() { return Attribute::Missing; }
        }
        match value {
            Some(Property::Null) => Attribute::Null,
            Some(Property::Bool(value)) => Attribute::Bool(*value),
            Some(Property::Number(value)) => Attribute::Number(*value),
            Some(Property::BigInt(value)) => Attribute::BigInt(value.as_str().into()),
            Some(Property::ObjectText(value)) => Attribute::Object(value.as_str().into()),
            Some(Property::Text(value)) => Attribute::String(value.as_str().into()),
            Some(Property::Node(_) | Property::Array(_) | Property::Record(_)) => Attribute::Object("[object Object]".into()),
            None => Attribute::Missing,
        }
    }
}

pub(super) fn matches(file: &NativeFile<'_>, node: u32, selector: &Selector) -> Result<bool, String> {
    file.cached(Projection::new).matches(node, selector)
}

pub(super) fn spans(file: &NativeFile<'_>, node: u32, selector: &Selector) -> Result<Vec<crate::selector::Span>, String> {
    file.cached(Projection::new).spans(node, selector)
}

struct Builder<'a, 't> {
    syntax: &'a Syntax<'t>,
    tokens: &'a [super::lexer::Token],
    projection: &'a mut Projection,
}

impl Builder<'_, '_> {
    fn add(&mut self, origin: u32, kind: &'static str, parent: Option<u32>, field: &'static str, position: Option<(usize, usize)>) -> u32 {
        self.projection.add(self.syntax, origin, kind, parent, field, position)
    }
    fn project(&mut self, origin: u32, parent: Option<u32>, field: &'static str, position: Option<(usize, usize)>, in_chain: bool) -> Option<u32> {
        let kind = self.syntax.kind(origin)?;
        if kind.is_token() || kind == K::Hole { return None; }
        if kind == K::JsxName {
            let native = self.syntax.tree.node(origin);
            return Some(self.jsx_name(origin, native.start, native.end, parent, field, position));
        }
        if matches!(kind, K::Paren | K::ParenType | K::Computed | K::Param) {
            let native_field = match kind { K::Paren | K::Computed => F::Expr, K::ParenType => F::Type, _ => F::Name };
            let child = self.syntax.get(origin, native_field)?;
            let property = kind == K::Param && self.syntax.all(origin, F::Modifier).next().is_some();
            let wrapper = property.then(|| self.add(origin, "TSParameterProperty", parent, field, position));
            let parent = wrapper.or(parent);
            let field = if property { "parameter" } else { field };
            let position = if property { None } else { position };
            let node = if kind == K::Param && self.syntax.get(origin, F::Init).is_some() {
                let id = self.add(origin, "AssignmentPattern", parent, field, position);
                self.projection.nodes[id as usize].start = self.syntax.tree.node(child).start;
                self.child(id, origin, F::Name, "left", false, false);
                self.child(id, origin, F::Init, "right", false, false);
                Some(id)
            } else { self.project(child, parent, field, position, false) }?;
            self.projection.native[origin as usize].push(node);
            if kind == K::Param {
                let target = if self.projection.kind(node) == "AssignmentPattern" {
                    self.projection.property(node, "left").and_then(|p| if let Property::Node(n) = p { Some(*n) } else { None }).unwrap_or(node)
                } else { node };
                self.annotation(target, origin, F::Type, "typeAnnotation");
                self.projection.set(target, "optional", Property::Bool(self.syntax.get(origin, F::Optional).is_some()));
            }
            if let Some(wrapper) = wrapper {
                self.projection.set(wrapper, "parameter", Property::Node(node));
                self.projection.set(wrapper, "readonly", Property::Bool(self.syntax.has_word(origin, F::Modifier, "readonly")));
                self.projection.set(wrapper, "override", Property::Bool(self.syntax.has_word(origin, F::Modifier, "override")));
                if let Some(accessibility) = ["public", "private", "protected"].into_iter().find(|word| self.syntax.has_word(origin, F::Modifier, word)) {
                    self.projection.set(wrapper, "accessibility", Property::Text(accessibility.into()));
                }
            }
            return Some(wrapper.unwrap_or(node));
        }
        if !in_chain && self.chain(origin) {
            let id = self.add(origin, "ChainExpression", parent, field, position);
            if let Some(inner) = self.project(origin, Some(id), "expression", None, true) {
                self.projection.set(id, "expression", Property::Node(inner));
            }
            return Some(id);
        }
        let descriptor = descriptor(kind)?;
        let jsx = parent.is_some_and(|parent| matches!(self.projection.kind(parent), "JSXOpeningElement" | "JSXClosingElement" | "JSXMemberExpression" | "JSXNamespacedName"));
        let name = match kind {
            K::Ident if jsx => "JSXIdentifier",
            K::Member if jsx => "JSXMemberExpression",
            K::Function => {
                if self.declaration(origin) {
                    if self.syntax.get(origin, F::Body).is_none() { "TSDeclareFunction" } else { "FunctionDeclaration" }
                } else if self.syntax.get(origin, F::Body).is_none() { "TSEmptyBodyFunctionExpression" } else { "FunctionExpression" }
            }
            K::Class if !self.declaration(origin) => "ClassExpression",
            K::Block if self.syntax.parent(origin).is_some_and(|parent| self.syntax.is(parent, K::Namespace)) => "TSModuleBlock",
            K::Method if self.syntax.parent(origin).is_some_and(|p| self.syntax.is(p, K::Object)) => "Property",
            K::TypeKeyword => keyword(self.syntax.text(origin)),
            K::Unary if self.syntax.word(origin, F::Op) == Some("...") => "RestElement",
            K::FnType if self.syntax.word(origin, F::Kind) == Some("new") => "TSConstructorType",
            K::CallSig if self.syntax.word(origin, F::Kind) == Some("new") => "TSConstructSignatureDeclaration",
            K::Export if self.syntax.has_word(origin, F::Kind, "default") => "ExportDefaultDeclaration",
            K::Export if self.exports_all(origin) => "ExportAllDeclaration",
            K::TypeRef if self.syntax.parent(origin).is_some_and(|parent| self.syntax.is(parent, K::Class)) && self.syntax.field(origin) == Some(F::Implements) => "TSClassImplements",
            K::TypeRef if self.syntax.parent(origin).is_some_and(|parent| self.syntax.is(parent, K::Interface)) && self.syntax.field(origin) == Some(F::Extends) => "TSInterfaceHeritage",
            _ => descriptor.name,
        };
        let id = self.add(origin, name, parent, field, position);
        self.attributes(id, origin, kind);
        for &(native, key, array) in descriptor.fields {
            let key = if matches!(name, "TSClassImplements" | "TSInterfaceHeritage") && native == F::Name { "expression" } else { key };
            self.child(id, origin, native, key, array, in_chain && matches!(native, F::Object | F::Callee));
        }
        if matches!(kind, K::PropSig | K::MethodSig | K::CallSig | K::IndexSig) {
            if let Some(token) = self.next_token(self.projection.nodes[id as usize].end) {
                if matches!(token.tok, super::lexer::Tok::P(";" | ",")) { self.projection.nodes[id as usize].end = token.end; }
            }
        }
        match kind {
            K::Method => {
                let body_kind = if self.syntax.get(origin, F::Body).is_some() { "FunctionExpression" } else { "TSEmptyBodyFunctionExpression" };
                let function = self.add(origin, body_kind, Some(id), "value", None);
                let start = self.syntax.get(origin, F::Key).map_or(self.projection.nodes[id as usize].start as usize, |key| self.syntax.tree.node(key).end as usize);
                let end = self.projection.nodes[id as usize].end as usize;
                if let Some(start) = self.token_range(start as u32, end as u32).iter().find(|token| matches!(token.tok, super::lexer::Tok::P("(" | "<"))).map(|token| token.start) {
                    self.projection.nodes[function as usize].start = start;
                }
                self.function(function, origin);
                self.projection.set(id, "value", Property::Node(function));
            }
            K::Function | K::Arrow | K::FnType | K::MethodSig | K::CallSig => self.function_metadata(id, origin),
            K::Class | K::Interface | K::Enum => {
                let body = self.add(origin, match kind { K::Class => "ClassBody", K::Enum => "TSEnumBody", _ => "TSInterfaceBody" }, Some(id), "body", None);
                let prefix_end = self.syntax.all(origin, F::Member).next().map_or(self.projection.nodes[id as usize].end as usize, |n| self.syntax.tree.node(n).start as usize);
                let start = self.projection.nodes[id as usize].start as usize;
                if let Some(start) = self.token_range(start as u32, prefix_end as u32).iter().rev().find(|token| token.tok == super::lexer::Tok::P("{")).map(|token| token.start) { self.projection.nodes[body as usize].start = start; }
                self.child(body, origin, F::Member, if kind == K::Enum { "members" } else { "body" }, true, false);
                self.projection.set(id, "body", Property::Node(body));
                self.parameters(id, origin, F::TypeParam, "typeParameters", "TSTypeParameterDeclaration");
                self.parameters(id, origin, F::TypeArg, "superTypeArguments", "TSTypeParameterInstantiation");
            }
            K::Try => {
                if let Some(handler) = self.syntax.get(origin, F::Handler) {
                    let catch = self.add(origin, "CatchClause", Some(id), "handler", None);
                    self.projection.nodes[catch as usize].start = self.syntax.get(origin, F::Kind).map_or(self.syntax.tree.node(handler).start, |n| self.syntax.tree.node(n).start);
                    self.projection.nodes[catch as usize].end = self.syntax.tree.node(handler).end;
                    self.child(catch, origin, F::Param, "param", false, false);
                    if let Some(Property::Node(param)) = self.projection.property(catch, "param") { self.annotation(*param, origin, F::Type, "typeAnnotation"); }
                    self.child(catch, origin, F::Handler, "body", false, false);
                    self.projection.set(id, "handler", Property::Node(catch));
                } else { self.projection.set(id, "handler", Property::Null); }
            }
            K::Import => self.imports(id, origin),
            K::TypeAlias => self.parameters(id, origin, F::TypeParam, "typeParameters", "TSTypeParameterDeclaration"),
            K::ImportSpec if self.syntax.get(origin, F::Local).is_none() => self.child(id, origin, F::Imported, "local", false, false),
            K::ExportSpec if self.syntax.get(origin, F::Exported).is_none() => self.child(id, origin, F::Local, "exported", false, false),
            K::AssignPattern if self.syntax.get(origin, F::Left).is_none() => {
                if let Some(property) = self.syntax.parent(origin).filter(|&parent| self.syntax.is(parent, K::PatternProp)) {
                    if let Some(key) = self.syntax.get(property, F::Key) { self.projection.nodes[id as usize].start = self.syntax.tree.node(key).start; }
                    self.child(id, property, F::Key, "left", false, false);
                }
            }
            K::Mapped => {
                if let Some(param) = self.syntax.get(origin, F::TypeParam) {
                    self.child(id, param, F::Name, "key", false, false);
                    self.child(id, param, F::Constraint, "constraint", false, false);
                }
            }
            K::Export if self.syntax.get(origin, F::Expr).is_some() => self.child(id, origin, F::Expr, "declaration", false, false),
            K::Export if self.exports_all(origin) => self.child(id, origin, F::Namespace, "exported", false, false),
            K::Prop | K::PatternProp if self.syntax.get(origin, F::Value).is_none() => {
                if let Some(key) = self.syntax.get(origin, F::Key) {
                    if let Some(value) = self.project(key, Some(id), "value", None, false) { self.projection.set(id, "value", Property::Node(value)); }
                }
            }
            K::MetaProperty => self.meta(id, origin),
            K::JsxElement | K::JsxFragment => self.jsx(id, origin, kind),
            K::Template | K::TemplateType => self.quasis(id, origin),
            _ => {}
        }
        if matches!(kind, K::Declarator | K::ClassProp | K::PropSig | K::IndexSig | K::ObjectPattern | K::ArrayPattern | K::Rest | K::TypePredicate | K::TupleMember) {
            let target = if kind == K::Declarator { self.projection.property(id, "id").and_then(|p| if let Property::Node(n) = p { Some(*n) } else { None }).unwrap_or(id) } else { id };
            self.annotation(target, origin, F::Type, "typeAnnotation");
        }
        if matches!(kind, K::Call | K::New | K::TaggedTemplate | K::Instantiation | K::TypeRef | K::TypeQuery | K::ImportType) {
            self.parameters(id, origin, F::TypeArg, "typeArguments", "TSTypeParameterInstantiation");
        }
        Some(id)
    }
    /// `export * from "m"` / `export * as ns from "m"` (`type` allowed).
    fn exports_all(&self, origin: u32) -> bool {
        let node = self.syntax.tree.node(origin);
        self.syntax.get(origin, F::From).is_some()
            && self.syntax.get(origin, F::Specifier).is_none()
            && self.token_range(node.start, node.end).iter().skip(1).take(2).any(|token| token.tok == super::lexer::Tok::P("*"))
    }
    fn declaration(&self, origin: u32) -> bool {
        self.syntax.parent(origin).is_some_and(|parent| {
            matches!(self.syntax.kind(parent), Some(K::Program | K::Block | K::Namespace))
                || self.syntax.is(parent, K::Export) && self.syntax.field(origin) == Some(F::Decl)
        })
    }
    fn chain(&self, mut origin: u32) -> bool {
        loop {
            let field = match self.syntax.kind(origin) { Some(K::Call) => F::Callee, Some(K::Member | K::Index) => F::Object, _ => return false };
            if self.syntax.get(origin, F::Optional).is_some() { return true; }
            match self.syntax.get(origin, field) { Some(child) => origin = child, None => return false }
        }
    }
    fn child(&mut self, parent: u32, origin: u32, native: F, key: &'static str, array: bool, in_chain: bool) {
        let children: Vec<_> = self.syntax.all(origin, native).collect();
        if array {
            let existing = match self.projection.property(parent, key) { Some(Property::Array(nodes)) => nodes.clone(), _ => vec![] };
            let len = existing.len() + children.len();
            let mut projected = existing;
            for child in children {
                projected.push(self.project(child, Some(parent), key, Some((projected.len(), len)), in_chain));
            }
            for (index, &child) in projected.iter().enumerate() {
                if let Some(child) = child { self.projection.nodes[child as usize].position = Some((index, len)); }
            }
            self.projection.set(parent, key, Property::Array(projected));
        } else {
            let child = children.first().and_then(|&child| self.project(child, Some(parent), key, None, in_chain));
            self.projection.set(parent, key, child.map_or(Property::Null, Property::Node));
        }
    }
    fn annotation(&mut self, parent: u32, origin: u32, native: F, key: &'static str) {
        let Some(annotation) = self.syntax.get(origin, native) else { return };
        let wrapper = self.add(annotation, "TSTypeAnnotation", Some(parent), key, None);
        let start = self.syntax.tree.node(annotation).start as usize;
        if let Some(token) = self.tokens[..self.tokens.partition_point(|token| (token.start as usize) < start)].last() {
            if matches!(token.tok, super::lexer::Tok::P(":" | "=>")) { self.projection.nodes[wrapper as usize].start = token.start; }
        }
        if let Some(child) = self.project(annotation, Some(wrapper), "typeAnnotation", None, false) { self.projection.set(wrapper, "typeAnnotation", Property::Node(child)); }
        self.projection.set(parent, key, Property::Node(wrapper));
        if self.projection.kind(parent) == "Identifier" { self.projection.nodes[parent as usize].end = self.syntax.tree.node(annotation).end; }
    }
    fn parameters(&mut self, parent: u32, origin: u32, native: F, key: &'static str, kind: &'static str) {
        let children: Vec<_> = self.syntax.all(origin, native).collect();
        let Some(&first) = children.first() else { return };
        let wrapper = self.add(first, kind, Some(parent), key, None);
        let first_start = self.syntax.tree.node(first).start as usize;
        if let Some(open) = self.syntax.src[..first_start].rfind('<') { self.projection.nodes[wrapper as usize].start = open as u32; }
        let end = self.syntax.tree.node(children.last().copied().unwrap_or(first)).end as usize;
        self.projection.nodes[wrapper as usize].end = self.syntax.src[end..].find('>').map_or(end, |close| end + close + 1) as u32;
        self.child(wrapper, origin, native, "params", true, false);
        self.projection.set(parent, key, Property::Node(wrapper));
    }
    fn function(&mut self, id: u32, origin: u32) {
        self.projection.set(id, "id", Property::Null);
        self.child(id, origin, F::Param, "params", true, false);
        self.child(id, origin, F::Body, "body", false, false);
        self.function_metadata(id, origin);
    }
    fn function_metadata(&mut self, id: u32, origin: u32) {
        self.parameters(id, origin, F::TypeParam, "typeParameters", "TSTypeParameterDeclaration");
        self.annotation(id, origin, F::ReturnType, "returnType");
        if !matches!(self.projection.kind(id), "FunctionDeclaration" | "FunctionExpression" | "ArrowFunctionExpression" | "TSDeclareFunction" | "TSEmptyBodyFunctionExpression") { return; }
        self.projection.set(id, "async", Property::Bool(self.syntax.has_word(origin, F::Modifier, "async")));
        self.projection.set(id, "generator", Property::Bool(self.syntax.has_word(origin, F::Modifier, "*")));
        self.projection.set(id, "expression", Property::Bool(self.syntax.get(origin, F::Body).is_some_and(|body| !self.syntax.is(body, K::Block))));
    }
    fn attributes(&mut self, id: u32, origin: u32, kind: K) {
        let text = self.syntax.text(origin);
        if matches!(kind, K::Ident | K::PrivateName | K::JsxName) {
            self.projection.set(id, "name", Property::Text(super::statics::identifier_name(text.trim_start_matches('#'))));
        }
        if matches!(kind, K::Null | K::Bool | K::Number | K::BigInt | K::String | K::Regex | K::JsxText) {
            self.projection.set(id, "raw", Property::Text(text.into()));
            let value = match kind {
                K::Null => Property::Null, K::Bool => Property::Bool(text == "true"),
                K::String => Property::Text(super::regex::cook(text).unwrap_or_else(|| text.into())),
                K::Number => {
                    Property::Number(super::statics::literal_value(self.syntax, origin).and_then(|(_, value)| value.parse().ok()).unwrap_or(f64::NAN))
                }
                K::BigInt => {
                    let value = super::statics::bigint_decimal(text).unwrap_or_else(|| text.trim_end_matches('n').into());
                    self.projection.set(id, "bigint", Property::Text(value.clone()));
                    Property::BigInt(value)
                }
                K::Regex => {
                    if let Some(end) = text.rfind('/') {
                        self.projection.set(id, "regex", Property::Record(vec![("pattern", Property::Text(text[1..end].into())), ("flags", Property::Text(text[end+1..].into()))]));
                    }
                    let flags: String = "dgimsuvy".chars().filter(|&flag| text[text.rfind('/').map_or(text.len(), |end| end + 1)..].contains(flag)).collect();
                    Property::ObjectText(format!("{}/{}", &text[..text.rfind('/').unwrap_or(text.len())], flags))
                }
                _ => Property::Text(text.into()),
            };
            self.projection.set(id, "value", value);
        }
        if matches!(kind, K::Member | K::Index | K::Call) && self.projection.kind(id) != "JSXMemberExpression" {
            self.projection.set(id, "optional", Property::Bool(self.syntax.get(origin, F::Optional).is_some()));
        }
        if matches!(kind, K::Member | K::Index | K::Prop | K::PatternProp | K::Method | K::ClassProp | K::PropSig | K::MethodSig) && self.projection.kind(id) != "JSXMemberExpression" {
            let computed = kind == K::Index || self.syntax.get(origin, F::Key).is_some_and(|key| self.syntax.is(key, K::Computed));
            self.projection.set(id, "computed", Property::Bool(computed));
        }
        if let Some(operator) = self.syntax.word(origin, F::Op) {
            self.projection.set(id, "operator", Property::Text(operator.into()));
        }
        if matches!(kind, K::Unary | K::Update) {
            self.projection.set(id, "prefix", Property::Bool(self.syntax.get(origin, F::Op).is_some_and(|op| self.syntax.tree.node(op).start == self.syntax.tree.node(origin).start)));
        }
        if kind == K::Var {
            self.projection.set(id, "kind", Property::Text(self.syntax.word(origin, F::Kind).unwrap_or("var").into()));
        }
        if matches!(kind, K::Prop | K::PatternProp | K::Method) {
            let accessor = self.syntax.word(origin, F::Kind).filter(|kind| matches!(*kind, "get" | "set"));
            let property = matches!(kind, K::Prop | K::PatternProp) || self.syntax.parent(origin).is_some_and(|parent| self.syntax.is(parent, K::Object));
            self.projection.set(id, "kind", Property::Text(accessor.unwrap_or(if property { "init" } else if self.syntax.get(origin, F::Key).is_some_and(|key| self.syntax.text(key) == "constructor") { "constructor" } else { "method" }).into()));
            if property {
                self.projection.set(id, "method", Property::Bool(kind == K::Method && accessor.is_none()));
                let shorthand = kind != K::Method && self.syntax.get(origin, F::Value).is_none_or(|value| self.syntax.is(value, K::AssignPattern) && self.syntax.get(value, F::Left).is_none());
                self.projection.set(id, "shorthand", Property::Bool(shorthand));
            }
        }
        if matches!(kind, K::Method | K::ClassProp) {
            self.projection.set(id, "static", Property::Bool(self.syntax.has_word(origin, F::Modifier, "static")));
        }
        if matches!(kind, K::ClassProp | K::PropSig | K::MethodSig) {
            self.projection.set(id, "optional", Property::Bool(self.syntax.get(origin, F::Optional).is_some()));
        }
        if kind == K::ForOf { self.projection.set(id, "await", Property::Bool(self.syntax.has_word(origin, F::Modifier, "await"))); }
        if kind == K::Yield { self.projection.set(id, "delegate", Property::Bool(self.syntax.word(origin, F::Op) == Some("*"))); }
        if kind == K::Program { self.projection.set(id, "sourceType", Property::Text(if self.syntax.all(origin, F::Body).any(|child| matches!(self.syntax.kind(child), Some(K::Import | K::Export))) { "module" } else { "script" }.into())); }
    }
    fn imports(&mut self, id: u32, origin: u32) {
        let mut specifiers = match self.projection.property(id, "specifiers") { Some(Property::Array(nodes)) => nodes.clone(), _ => vec![] };
        for (field, kind) in [(F::DefaultImport, "ImportDefaultSpecifier"), (F::Namespace, "ImportNamespaceSpecifier")] {
            for local in self.syntax.all(origin, field).collect::<Vec<_>>() {
                let specifier = self.add(local, kind, Some(id), "specifiers", None);
                if field == F::Namespace {
                    let start = self.syntax.tree.node(origin).start;
                    let end = self.syntax.tree.node(local).start;
                    if let Some(start) = self.token_range(start, end).iter().rev().find(|token| token.tok == super::lexer::Tok::P("*")).map(|token| token.start) { self.projection.nodes[specifier as usize].start = start; }
                }
                if let Some(child) = self.project(local, Some(specifier), "local", None, false) { self.projection.set(specifier, "local", Property::Node(child)); }
                specifiers.push(Some(specifier));
            }
        }
        specifiers.sort_by_key(|node| node.map_or(0, |node| self.projection.nodes[node as usize].start));
        for (index, &node) in specifiers.iter().enumerate() { if let Some(node) = node { self.projection.nodes[node as usize].position = Some((index, specifiers.len())); } }
        self.projection.set(id, "specifiers", Property::Array(specifiers));
    }
    fn jsx(&mut self, id: u32, origin: u32, kind: K) {
        let open_field = if kind == K::JsxElement { "openingElement" } else { "openingFragment" };
        let open = self.add(origin, if kind == K::JsxElement { "JSXOpeningElement" } else { "JSXOpeningFragment" }, Some(id), open_field, None);
        let start = self.syntax.tree.node(origin).start as usize;
        let text = self.syntax.text(origin);
        if let Some(end) = text.find('>') { self.projection.nodes[open as usize].end = (start + end + 1) as u32; }
        self.child(open, origin, F::Name, "name", false, false);
        self.child(open, origin, F::Property, "attributes", true, false);
        self.projection.set(open, "selfClosing", Property::Bool(text[..(self.projection.nodes[open as usize].end as usize - start)].ends_with("/>")));
        self.projection.set(id, open_field, Property::Node(open));
        if self.syntax.get(origin, F::Closing).is_some() || kind == K::JsxFragment {
            let close_field = if kind == K::JsxElement { "closingElement" } else { "closingFragment" };
            let close = self.add(origin, if kind == K::JsxElement { "JSXClosingElement" } else { "JSXClosingFragment" }, Some(id), close_field, None);
            if let Some(offset) = text.rfind("</") { self.projection.nodes[close as usize].start = (start + offset) as u32; }
            self.child(close, origin, F::Closing, "name", false, false);
            self.projection.set(id, close_field, Property::Node(close));
        }
    }
    fn jsx_name(&mut self, origin: u32, start: u32, end: u32, parent: Option<u32>, field: &'static str, position: Option<(usize, usize)>) -> u32 {
        let text = &self.syntax.src[start as usize..end as usize];
        let split = text.rfind('.').map(|at| (at, "JSXMemberExpression", "object", "property"))
            .or_else(|| text.find(':').map(|at| (at, "JSXNamespacedName", "namespace", "name")));
        let id = self.add(origin, split.map_or("JSXIdentifier", |(_, kind, _, _)| kind), parent, field, position);
        self.projection.nodes[id as usize].start = start;
        self.projection.nodes[id as usize].end = end;
        if let Some((at, _, left, right)) = split {
            let left_node = self.jsx_name(origin, start, start + at as u32, Some(id), left, None);
            let right_node = self.jsx_name(origin, start + at as u32 + 1, end, Some(id), right, None);
            self.projection.set(id, left, Property::Node(left_node));
            self.projection.set(id, right, Property::Node(right_node));
        } else {
            self.projection.set(id, "name", Property::Text(text.to_owned()));
        }
        id
    }
    fn next_token(&self, at: u32) -> Option<super::lexer::Token> {
        self.tokens.get(self.tokens.partition_point(|token| token.start < at)).copied()
    }
    fn token_range(&self, start: u32, end: u32) -> &[super::lexer::Token] {
        &self.tokens[self.tokens.partition_point(|token| token.start < start)..self.tokens.partition_point(|token| token.start < end)]
    }
    fn meta(&mut self, id: u32, origin: u32) {
        let span = self.syntax.tree.node(origin);
        let names: Vec<_> = self.token_range(span.start, span.end).iter().filter(|token| token.tok == super::lexer::Tok::Ident).copied().collect();
        for (&token, field) in names.iter().zip(["meta", "property"]) {
            let child = self.add(origin, "Identifier", Some(id), field, None);
            self.projection.nodes[child as usize].start = token.start;
            self.projection.nodes[child as usize].end = token.end;
            self.projection.set(child, "name", Property::Text(self.syntax.src[token.start as usize..token.end as usize].into()));
            self.projection.set(id, field, Property::Node(child));
        }
    }
    fn quasis(&mut self, id: u32, origin: u32) {
        let quasis: Vec<_> = self.syntax.all(origin, F::Quasi).collect();
        let mut projected = vec![];
        for (index, &quasi) in quasis.iter().enumerate() {
            let node = self.add(quasi, "TemplateElement", Some(id), "quasis", Some((index, quasis.len())));
            self.projection.set(node, "tail", Property::Bool(index + 1 == quasis.len()));
            self.projection.set(node, "value", Property::Record(vec![("raw", Property::Text(self.syntax.text(quasi).into())), ("cooked", Property::Text(self.syntax.text(quasi).into()))]));
            projected.push(Some(node));
        }
        self.projection.set(id, "quasis", Property::Array(projected));
    }
}

fn keyword(text: &str) -> &'static str {
    match text {
        "any" => "TSAnyKeyword", "bigint" => "TSBigIntKeyword", "boolean" => "TSBooleanKeyword", "intrinsic" => "TSIntrinsicKeyword",
        "never" => "TSNeverKeyword", "null" => "TSNullKeyword", "number" => "TSNumberKeyword", "object" => "TSObjectKeyword",
        "string" => "TSStringKeyword", "symbol" => "TSSymbolKeyword", "undefined" => "TSUndefinedKeyword", "unknown" => "TSUnknownKeyword", "void" => "TSVoidKeyword",
        _ => "TSThisType",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;
    use crate::engine::tests::native_linter;
    use std::path::Path;

    fn spans(source: &str, selector: &str) -> Vec<String> {
        let mut out = vec![];
        with_native_file(source, |file, _| {
            let tree = Projection::new(file);
            let selector = crate::selector::compile(selector).unwrap();
            for node in tree.nodes() {
                if selector.matches(&tree, node).unwrap() {
                    out.push(tree.kind(node).to_owned());
                }
            }
        });
        out
    }

    #[test]
    fn virtual_nodes_are_distinct_selector_targets() {
        assert_eq!(spans("class C { method() {} }", "MethodDefinition, FunctionExpression"), ["MethodDefinition", "FunctionExpression"]);
        assert_eq!(spans("a?.()", "ChainExpression, CallExpression"), ["ChainExpression", "CallExpression"]);
    }

    #[test]
    fn fields_attributes_and_contextual_kinds_follow_estree() {
        assert_eq!(spans("const value = function named() {}; function declared() {}", "FunctionExpression[id.name=named], FunctionDeclaration[id.name=declared]"), ["FunctionExpression", "FunctionDeclaration"]);
        assert_eq!(spans("[first, , last];", "Identifier.elements:last-child"), ["Identifier"]);
        assert_eq!(spans("({ method() {}, value: 2 })", "Property > FunctionExpression.value"), ["FunctionExpression"]);
    }

    #[test]
    fn selector_reports_keep_distinct_virtual_spans() {
        let linter = native_linter("in ts_program rule methods {\nreport at first(selector_spans(\"MethodDefinition, FunctionExpression\"))\nwarn \"method\"\n}\nin ts_program rule functions {\nreport at last(selector_spans(\"MethodDefinition, FunctionExpression\"))\nwarn \"function\"\n}\nin ts_program rule names {\nreport at first(selector_spans(\"Identifier[name=method]\"))\nwarn \"blocked\"\n}");
        let source = "class C { method() {} }";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), source);
        let mut ranges: Vec<_> = diagnostics.iter().map(|d| (d.start, d.end)).collect();
        ranges.sort();
        assert_eq!(ranges, [(10, 16), (10, 21), (16, 21)]);
        assert_eq!(diagnostics.iter().filter(|d| d.message == "blocked").count(), 1);
        assert!(diagnostics.iter().all(|d| d.fix.is_none()));
    }

    #[test]
    fn malformed_selectors_become_rule_errors_not_silent_misses() {
        let linter = native_linter("in ts_program rule probe {\neach hit in selector_spans(\"Identifier[\")\nwarn \"unexpected\"\n}");
        let diagnostics = linter.lint(Path::new("/root/a.ts"), "value;");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("failed"));
        assert!(diagnostics[0].message.contains("selector"));
    }

    fn selected_text(source: &str, selector: &str) -> Vec<String> {
        let mut result = vec![];
        with_native_file(source, |file, _| {
            let tree = Projection::new(file);
            let selector = crate::selector::compile(selector).unwrap();
            result = tree.spans(file.syntax.root(), &selector).unwrap().into_iter()
                .map(|span| source[span.start as usize..span.end as usize].to_owned()).collect();
        });
        result
    }

    #[test]
    fn annotations_belong_to_parameter_identifiers_and_extend_their_spans() {
        let source = "function f(first: number = 2, second: string) {} try {} catch (error: unknown) {}";
        assert_eq!(selected_text(source, "Identifier[typeAnnotation]"), ["first: number", "second: string", "error: unknown"]);
    }

    #[test]
    fn shorthand_and_meta_properties_keep_both_logical_children() {
        assert_eq!(spans("({value})", "Property > Identifier"), ["Identifier", "Identifier"]);
        assert_eq!(spans("({value})", "Property > Identifier.value"), ["Identifier"]);
        assert_eq!(selected_text("function f() { return new.target; } const m = import.meta;", "MetaProperty > Identifier"), ["new", "target", "import", "meta"]);
    }

    #[test]
    fn heritage_import_export_and_jsx_use_contextual_roles() {
        assert_eq!(spans("class C implements I<number> {}", "TSClassImplements > Identifier.expression"), ["Identifier"]);
        assert_eq!(spans("export default () => 2;", "ExportDefaultDeclaration > ArrowFunctionExpression.declaration"), ["ArrowFunctionExpression"]);
        assert_eq!(selected_text("import * as ns from 'mod';", "ImportNamespaceSpecifier"), ["* as ns"]);
        assert_eq!(selected_text("export * as ns from 'm' with { type: 'json' }; export {} from 'n';", "ExportAllDeclaration > Identifier.exported"), ["ns"]);
        assert_eq!(selected_text("export type * from 'm'; export {} from 'n';", "ExportNamedDeclaration"), ["export {} from 'n';"]);
    }

    #[test]
    fn jsx_names_and_fragment_closers_are_logical_targets() {
        let selected = |source: &str, selector: &str| {
            let config = format!("in ts_program rule probe {{\neach hit in selector_spans({selector:?})\nreport at hit\nwarn byte_slice(source, hit.start, hit.end)\n}}");
            native_linter(&config).lint(Path::new("/root/a.tsx"), source).into_iter().map(|diagnostic| diagnostic.message).collect::<Vec<_>>()
        };
        assert_eq!(selected("const x = <Foo.Bar />;", "JSXMemberExpression > JSXIdentifier"), ["Foo", "Bar"]);
        assert_eq!(selected("const x = <>text</>;", "JSXClosingFragment"), ["</>"]);
    }

    #[test]
    fn numeric_attribute_literals_use_javascript_string_conversion() {
        assert_eq!(selected_text("'1e+21'; 2;", "Literal[value=1000000000000000000000], Literal[value=02]"), ["'1e+21'", "2"]);
    }

    #[test]
    fn indexed_attributes_preserve_node_and_null_slots() {
        assert_eq!(selected_text("call(value); empty();", "CallExpression[arguments.0]"), ["call(value)"]);
        assert_eq!(selected_text("[first, , last]; [first];", "ArrayExpression[elements.1=type(object)]"), ["[first, , last]"]);
        assert_eq!(selected_text("9007199254740993n;", "Literal[value>'9007199254740992']"), ["9007199254740993n"]);
    }
}
