//! The node kinds of the TypeScript front end, and the fields each kind can
//! have. Rules see a kind as the layer `ts_<name>` and its fields as fields.
//! Names follow ESTree where it has one.

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
    Body = "body", Expr = "expr", Kind = "kind", Decl = "decl", Name = "name", Type = "type", Init = "init",
    Test = "test", Consequent = "consequent", Alternate = "alternate", Update = "update", Left = "left",
    Right = "right", Arg = "arg", Label = "label", Discriminant = "discriminant", Case = "case", Block = "block",
    Param = "param", Handler = "handler", Finalizer = "finalizer", Object = "object", Key = "key", Value = "value",
    TypeParam = "type_param", ReturnType = "return_type", Extends = "extends", Implements = "implements",
    Member = "member", Property = "property", Index = "index", Callee = "callee", TypeArg = "type_arg", Op = "op",
    Element = "element", Prop = "prop", Rest = "rest", From = "from", Specifier = "specifier",
    DefaultImport = "default_import", Namespace = "namespace", Imported = "imported", Local = "local",
    Exported = "exported", Quasi = "quasi", Tag = "tag", Constraint = "constraint", Default = "default",
    Check = "check", Optional = "optional", Modifier = "modifier", Decorator = "decorator", Qualifier = "qualifier",
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
    ExprStmt = "ts_expr_stmt" [Expr],
    Var = "ts_var" [Kind, Decl, Modifier],
    Declarator = "ts_declarator" [Name, Type, Init],
    If = "ts_if" [Test, Consequent, Alternate],
    For = "ts_for" [Init, Test, Update, Body],
    ForIn = "ts_for_in" [Left, Right, Body],
    ForOf = "ts_for_of" [Left, Right, Body],
    While = "ts_while" [Test, Body],
    DoWhile = "ts_do_while" [Body, Test],
    Return = "ts_return" [Arg],
    Throw = "ts_throw" [Arg],
    Break = "ts_break" [Label],
    Continue = "ts_continue" [Label],
    Switch = "ts_switch" [Discriminant, Case],
    Case = "ts_case" [Test, Body],
    Try = "ts_try" [Block, Param, Handler, Finalizer],
    Labeled = "ts_labeled" [Label, Body],
    Empty = "ts_empty" [],
    Debugger = "ts_debugger" [],
    With = "ts_with" [Object, Body],
    // ---- functions and classes
    Function = "ts_function" [Modifier, Name, TypeParam, Param, ReturnType, Body],
    Arrow = "ts_arrow" [Modifier, TypeParam, Param, ReturnType, Body],
    Param = "ts_param" [Decorator, Modifier, Name, Optional, Type, Init],
    Class = "ts_class" [Decorator, Modifier, Name, TypeParam, Extends, TypeArg, Implements, Member],
    Method = "ts_method" [Decorator, Modifier, Kind, Key, Optional, TypeParam, Param, ReturnType, Body],
    ClassProp = "ts_class_prop" [Decorator, Modifier, Key, Optional, Type, Init],
    StaticBlock = "ts_static_block" [Body],
    IndexSig = "ts_index_sig" [Modifier, Param, Type],
    Decorator = "ts_decorator" [Expr],
    // ---- modules
    Import = "ts_import" [Kind, DefaultImport, Namespace, Specifier, From],
    ImportSpec = "ts_import_spec" [Kind, Imported, Local],
    ImportEquals = "ts_import_equals" [Modifier, Name, Value],
    Export = "ts_export" [Kind, Decl, Specifier, Namespace, From, Expr],
    ExportSpec = "ts_export_spec" [Kind, Local, Exported],
    ExportAssign = "ts_export_assign" [Expr],
    // ---- type declarations
    Interface = "ts_interface" [Modifier, Name, TypeParam, Extends, Member],
    TypeAlias = "ts_type_alias" [Modifier, Name, TypeParam, Type],
    Enum = "ts_enum" [Modifier, Name, Member],
    EnumMember = "ts_enum_member" [Name, Init],
    Namespace = "ts_namespace" [Modifier, Kind, Name, Body],
    // ---- patterns
    ObjectPattern = "ts_object_pattern" [Prop, Rest, Type],
    ArrayPattern = "ts_array_pattern" [Element, Type],
    AssignPattern = "ts_assign_pattern" [Left, Right],
    PatternProp = "ts_pattern_prop" [Key, Value],
    Rest = "ts_rest" [Arg, Type],
    Hole = "ts_hole" [],
    // ---- expressions
    Ident = "ts_ident" [],
    PrivateName = "ts_private_name" [],
    This = "ts_this" [],
    Super = "ts_super" [],
    Null = "ts_null" [],
    Bool = "ts_bool" [],
    Number = "ts_number" [],
    BigInt = "ts_bigint" [],
    String = "ts_string" [],
    Regex = "ts_regex" [],
    Template = "ts_template" [Quasi, Expr],
    TaggedTemplate = "ts_tagged_template" [Tag, TypeArg, Quasi],
    Array = "ts_array" [Element],
    Object = "ts_object" [Prop],
    Prop = "ts_prop" [Key, Value],
    // `[expr]` as a property name.
    Computed = "ts_computed" [Expr],
    Spread = "ts_spread" [Arg],
    Member = "ts_member" [Object, Optional, Property],
    Index = "ts_index" [Object, Optional, Index],
    Call = "ts_call" [Callee, Optional, TypeArg, Arg],
    New = "ts_new" [Callee, TypeArg, Arg],
    Unary = "ts_unary" [Op, Arg],
    Update = "ts_update" [Op, Arg],
    Await = "ts_await" [Arg],
    Yield = "ts_yield" [Arg],
    Binary = "ts_binary" [Left, Op, Right],
    Logical = "ts_logical" [Left, Op, Right],
    Assign = "ts_assign" [Left, Op, Right],
    Conditional = "ts_conditional" [Test, Consequent, Alternate],
    Sequence = "ts_sequence" [Expr],
    Paren = "ts_paren" [Expr],
    As = "ts_as" [Expr, Type],
    Satisfies = "ts_satisfies" [Expr, Type],
    NonNull = "ts_non_null" [Expr],
    TypeAssertion = "ts_type_assertion" [Type, Expr],
    MetaProperty = "ts_meta_property" [],
    ImportCall = "ts_import_call" [Arg],
    Instantiation = "ts_instantiation" [Expr, TypeArg],
    // ---- types
    TypeRef = "ts_type_ref" [Name, TypeArg],
    QualifiedName = "ts_qualified_name" [Left, Right],
    TypeParam = "ts_type_param" [Modifier, Name, Constraint, Default],
    TypeKeyword = "ts_type_keyword" [],
    Union = "ts_union" [Type],
    Intersection = "ts_intersection" [Type],
    ArrayType = "ts_array_type" [Type],
    TupleType = "ts_tuple_type" [Element],
    TupleMember = "ts_tuple_member" [Name, Optional, Type],
    FnType = "ts_fn_type" [Kind, TypeParam, Param, ReturnType],
    TypeLit = "ts_type_lit" [Member],
    PropSig = "ts_prop_sig" [Modifier, Key, Optional, Type],
    MethodSig = "ts_method_sig" [Kind, Key, Optional, TypeParam, Param, ReturnType],
    CallSig = "ts_call_sig" [Kind, TypeParam, Param, ReturnType],
    TypeQuery = "ts_type_query" [Name, TypeArg],
    TypeOperator = "ts_type_operator" [Op, Type],
    IndexedAccess = "ts_indexed_access" [Object, Index],
    ConditionalType = "ts_conditional_type" [Check, Extends, Consequent, Alternate],
    Infer = "ts_infer" [TypeParam],
    Mapped = "ts_mapped" [Modifier, TypeParam, Name, Optional, Type],
    ParenType = "ts_paren_type" [Type],
    TemplateType = "ts_template_type" [Quasi, Type],
    TypePredicate = "ts_type_predicate" [Modifier, Name, Type],
    ImportType = "ts_import_type" [Arg, Qualifier, TypeArg],
    LiteralType = "ts_literal_type" [Value],
}

impl Kind {
    pub fn name(self) -> &'static str {
        KINDS[self as usize].0
    }
}
