//! The node kinds of the TypeScript front end, and the fields each kind can
//! have. Rules see a kind as the layer `ts_<name>` and its fields as fields.
//! Names follow ESTree where it has one, and are whole words otherwise.

macro_rules! fields {
    ($($v:ident = $s:literal),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u16)]
        pub enum Field { $($v),* }
        pub const FIELD_NAMES: &[&str] = &[$($s),*];
        pub const ALL_FIELDS: &[Field] = &[$(Field::$v),*];
    };
}

fields! {
    Body = "body", Expr = "expression", Kind = "kind", Decl = "declaration", Name = "name", Type = "type", Init = "init",
    Test = "test", Consequent = "consequent", Alternate = "alternate", Update = "update", Left = "left",
    Right = "right", Arg = "argument", Label = "label", Discriminant = "discriminant", Case = "case", Block = "block",
    Param = "parameter", Handler = "handler", Finalizer = "finalizer", Object = "object", Key = "key", Value = "value",
    TypeParam = "type_parameter", ReturnType = "return_type", Extends = "extends", Implements = "implements",
    Member = "member", Property = "property", Index = "index", Callee = "callee", TypeArg = "type_argument", Op = "operator",
    Element = "element", Rest = "rest", From = "from", Specifier = "specifier",
    DefaultImport = "default_import", Namespace = "namespace", Imported = "imported", Local = "local",
    Exported = "exported", Quasi = "quasi", Tag = "tag", Constraint = "constraint", Default = "default",
    Check = "check", Optional = "optional", Modifier = "modifier", Decorator = "decorator", Qualifier = "qualifier",
    Closing = "closing",
    // The node's own punctuation and keyword tokens (see `tokens::attach`).
    Token = "token",
}

macro_rules! kinds {
    ($($v:ident = $s:literal [$($f:ident),*]),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u16)]
        pub enum Kind { $($v),* }
        /// `(layer name, fields)` for every kind, indexed by `Kind as usize`.
        pub const KINDS: &[(&str, &[Field])] = &[$(($s, &[$(Field::$f),*])),*];
        pub const ALL_KINDS: &[Kind] = &[$(Kind::$v),*];
    };
}

kinds! {
    Program = "ts_program" [Body],
    // ---- statements
    Block = "ts_block" [Body],
    ExprStmt = "ts_expression_statement" [Expr],
    Var = "ts_variable_declaration" [Kind, Decl, Modifier],
    Declarator = "ts_declarator" [Name, Type, Init],
    If = "ts_if" [Test, Consequent, Alternate],
    For = "ts_for" [Init, Test, Update, Body],
    ForIn = "ts_for_in" [Left, Right, Body],
    // `modifier`: the `await` of `for await`.
    ForOf = "ts_for_of" [Modifier, Left, Right, Body],
    While = "ts_while" [Test, Body],
    DoWhile = "ts_do_while" [Body, Test],
    Return = "ts_return" [Arg],
    Throw = "ts_throw" [Arg],
    Break = "ts_break" [Label],
    Continue = "ts_continue" [Label],
    Switch = "ts_switch" [Discriminant, Case],
    Case = "ts_case" [Test, Body],
    // `kind`: the `catch` keyword.
    Try = "ts_try" [Block, Kind, Param, Handler, Finalizer],
    Labeled = "ts_labeled" [Label, Body],
    Empty = "ts_empty" [],
    Debugger = "ts_debugger" [],
    With = "ts_with" [Object, Body],
    // ---- functions and classes
    Function = "ts_function" [Modifier, Name, TypeParam, Param, ReturnType, Body],
    // `operator`: the `=>` token.
    Arrow = "ts_arrow" [Modifier, TypeParam, Param, ReturnType, Op, Body],
    Param = "ts_parameter" [Decorator, Modifier, Name, Optional, Type, Init],
    Class = "ts_class" [Decorator, Modifier, Name, TypeParam, Extends, TypeArg, Implements, Member],
    Method = "ts_method" [Decorator, Modifier, Kind, Key, Optional, TypeParam, Param, ReturnType, Body],
    ClassProp = "ts_class_property" [Decorator, Modifier, Key, Optional, Type, Init],
    StaticBlock = "ts_static_block" [Body],
    IndexSig = "ts_index_signature" [Modifier, Param, Type],
    Decorator = "ts_decorator" [Expr],
    // ---- modules
    Import = "ts_import" [Kind, DefaultImport, Namespace, Specifier, From],
    ImportSpec = "ts_import_specifier" [Kind, Imported, Local],
    // `kind`: `type` for `import type x = require(...)`.
    ImportEquals = "ts_import_equals" [Kind, Modifier, Name, Value],
    Export = "ts_export" [Kind, Decl, Specifier, Namespace, From, Expr],
    ExportSpec = "ts_export_specifier" [Kind, Local, Exported],
    ExportAssign = "ts_export_assignment" [Expr],
    // ---- type declarations
    Interface = "ts_interface" [Modifier, Name, TypeParam, Extends, Member],
    TypeAlias = "ts_type_alias" [Modifier, Name, TypeParam, Type],
    Enum = "ts_enum" [Modifier, Name, Member],
    EnumMember = "ts_enum_member" [Name, Init],
    Namespace = "ts_namespace" [Modifier, Kind, Name, Body],
    // ---- patterns
    ObjectPattern = "ts_object_pattern" [Property, Rest, Type],
    ArrayPattern = "ts_array_pattern" [Element, Type],
    AssignPattern = "ts_assignment_pattern" [Left, Right],
    PatternProp = "ts_pattern_property" [Key, Value],
    Rest = "ts_rest" [Arg, Type],
    Hole = "ts_hole" [],
    // ---- expressions
    Ident = "ts_identifier" [],
    PrivateName = "ts_private_name" [],
    This = "ts_this" [],
    Super = "ts_super" [],
    Null = "ts_null" [],
    Bool = "ts_boolean" [],
    Number = "ts_number" [],
    BigInt = "ts_bigint" [],
    String = "ts_string" [],
    Regex = "ts_regex" [],
    Template = "ts_template" [Quasi, Expr],
    TaggedTemplate = "ts_tagged_template" [Tag, TypeArg, Quasi],
    Array = "ts_array" [Element],
    Object = "ts_object" [Property],
    Prop = "ts_property" [Key, Value],
    // `[expression]` as a property name.
    Computed = "ts_computed_key" [Expr],
    Spread = "ts_spread" [Arg],
    Member = "ts_member" [Object, Optional, Property],
    Index = "ts_index" [Object, Optional, Index],
    Call = "ts_call" [Callee, Optional, TypeArg, Arg],
    New = "ts_new" [Callee, TypeArg, Arg],
    Unary = "ts_unary" [Op, Arg],
    Update = "ts_update" [Op, Arg],
    Await = "ts_await" [Arg],
    // `operator`: the `*` of `yield*`.
    Yield = "ts_yield" [Op, Arg],
    Binary = "ts_binary" [Left, Op, Right],
    Logical = "ts_logical" [Left, Op, Right],
    Assign = "ts_assignment" [Left, Op, Right],
    Conditional = "ts_conditional" [Test, Consequent, Alternate],
    Sequence = "ts_sequence" [Expr],
    Paren = "ts_parenthesized" [Expr],
    As = "ts_as" [Expr, Type],
    Satisfies = "ts_satisfies" [Expr, Type],
    NonNull = "ts_non_null" [Expr],
    TypeAssertion = "ts_type_assertion" [Type, Expr],
    MetaProperty = "ts_meta_property" [],
    ImportCall = "ts_import_call" [Arg],
    Instantiation = "ts_instantiation" [Expr, TypeArg],
    // ---- JSX
    // `name`: a component (`Foo`, `a.b`: identifiers and members, which
    // reference bindings) or a `ts_jsx_name` (`div`, `a-b`, `a:b`), as
    // eslint-scope sees them; `property`: attributes; `body`: children;
    // `closing`: the closing tag's name.
    JsxElement = "ts_jsx_element" [Name, TypeArg, Property, Body, Closing],
    JsxFragment = "ts_jsx_fragment" [Body],
    JsxAttribute = "ts_jsx_attribute" [Name, Value],
    JsxSpreadAttribute = "ts_jsx_spread_attribute" [Arg],
    // `{expression}` as a child or attribute value; `{}` and `{/* c */}`
    // have no expression.
    JsxExpression = "ts_jsx_expression" [Expr],
    JsxText = "ts_jsx_text" [],
    JsxName = "ts_jsx_name" [],
    // ---- tokens: punctuation and keywords no child node covers, under the
    // node that owns them (field `token`); built only when a rule needs them
    Comma = "ts_comma" [],
    Semicolon = "ts_semicolon" [],
    Colon = "ts_colon" [],
    Dot = "ts_dot" [],
    Question = "ts_question" [],
    Ellipsis = "ts_ellipsis" [],
    Equals = "ts_equals" [],
    OpenParen = "ts_open_paren" [],
    CloseParen = "ts_close_paren" [],
    OpenBracket = "ts_open_bracket" [],
    CloseBracket = "ts_close_bracket" [],
    OpenBrace = "ts_open_brace" [],
    CloseBrace = "ts_close_brace" [],
    OpenAngle = "ts_open_angle" [],
    CloseAngle = "ts_close_angle" [],
    Keyword = "ts_keyword" [],
    Punctuator = "ts_punctuator" [],
    // ---- types (every kind from here on is a type)
    TypeRef = "ts_type_reference" [Name, TypeArg],
    QualifiedName = "ts_qualified_name" [Left, Right],
    TypeParam = "ts_type_parameter" [Modifier, Name, Constraint, Default],
    TypeKeyword = "ts_type_keyword" [],
    Union = "ts_union" [Type],
    Intersection = "ts_intersection" [Type],
    ArrayType = "ts_array_type" [Type],
    TupleType = "ts_tuple_type" [Element],
    TupleMember = "ts_tuple_member" [Name, Optional, Type],
    FnType = "ts_function_type" [Kind, TypeParam, Param, ReturnType],
    TypeLit = "ts_type_literal" [Member],
    PropSig = "ts_property_signature" [Modifier, Key, Optional, Type],
    MethodSig = "ts_method_signature" [Kind, Key, Optional, TypeParam, Param, ReturnType],
    CallSig = "ts_call_signature" [Kind, TypeParam, Param, ReturnType],
    TypeQuery = "ts_type_query" [Name, TypeArg],
    TypeOperator = "ts_type_operator" [Op, Type],
    IndexedAccess = "ts_indexed_access" [Object, Index],
    ConditionalType = "ts_conditional_type" [Check, Extends, Consequent, Alternate],
    Infer = "ts_infer" [TypeParam],
    Mapped = "ts_mapped_type" [Modifier, TypeParam, Name, Optional, Type],
    ParenType = "ts_parenthesized_type" [Type],
    TemplateType = "ts_template_type" [Quasi, Type],
    TypePredicate = "ts_type_predicate" [Modifier, Name, Type],
    ImportType = "ts_import_type" [Arg, Qualifier, TypeArg],
    LiteralType = "ts_literal_type" [Value],
}

impl Kind {
    pub fn name(self) -> &'static str {
        KINDS[self as usize].0
    }

    /// A token node (`ts_comma`, `ts_keyword`, ...), not syntax.
    pub fn is_token(self) -> bool {
        Self::is_token_index(self as usize)
    }

    /// Whether `KINDS[i]` is a token kind.
    pub fn is_token_index(i: usize) -> bool {
        (Kind::Comma as usize..=Kind::Punctuator as usize).contains(&i)
    }
}
