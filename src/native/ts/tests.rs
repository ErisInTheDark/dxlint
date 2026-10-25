use super::kinds::{Kind, ALL_KINDS, FIELD_NAMES};
use super::parse;
use crate::grammar::TEXT;

/// `kind(field=child ...)` with leaves as their text, for compact asserts.
fn dump(src: &str) -> String {
    dump_as(src, false)
}

fn dump_as(src: &str, jsx: bool) -> String {
    let p = parse(src, jsx).unwrap_or_else(|(at, e)| panic!("{e} at {at}: {}", &src[at as usize..]));
    super::kinds::validate_shapes(&p);
    let mut out = String::new();
    let root = &p.nodes[p.root as usize];
    for (i, e) in root.children.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        write(&p.nodes, src, e.node, &mut out);
    }
    out
}

fn write(nodes: &[crate::grammar::Node], src: &str, id: u32, out: &mut String) {
    let n = &nodes[id as usize];
    let text = &src[n.start as usize..n.end as usize];
    if n.matcher == TEXT || n.children.is_empty() {
        out.push_str(text);
        return;
    }
    let kind = ALL_KINDS[n.matcher as usize];
    out.push_str(&kind.name()[3..]);
    out.push('(');
    for (i, e) in n.children.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(FIELD_NAMES[e.label as usize]);
        out.push('=');
        write(nodes, src, e.node, out);
    }
    out.push(')');
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(
        dump("a || b && c;"),
        "expression_statement(expression=logical(left=a operator=|| right=logical(left=b operator=&& right=c)))"
    );
    assert_eq!(
        dump("a ** b ** c;"),
        "expression_statement(expression=binary(left=a operator=** right=binary(left=b operator=** right=c)))"
    );
    assert_eq!(
        dump("x = a >> b >= c;"),
        "expression_statement(expression=assignment(left=x operator== right=binary(left=binary(left=a operator=>> right=b) operator=>= right=c)))"
    );
    assert_eq!(dump("x >>>= 1;"), "expression_statement(expression=assignment(left=x operator=>>>= right=1))");
}

#[test]
fn arrows_generics_and_casts() {
    assert_eq!(
        dump("f<T>(x);"),
        "expression_statement(expression=call(callee=f type_argument=type_reference(name=T) argument=x))"
    );
    assert_eq!(
        dump("a < b > c;"),
        "expression_statement(expression=binary(left=binary(left=a operator=< right=b) operator=> right=c))"
    );
    assert_eq!(
        dump("(a, b): T => a;"),
        "expression_statement(expression=arrow(parameter=parameter(name=a) parameter=parameter(name=b) return_type=type_reference(name=T) operator==> body=a))"
    );
    assert_eq!(
        dump("async x => x;"),
        "expression_statement(expression=arrow(modifier=async parameter=parameter(name=x) operator==> body=x))"
    );
    assert_eq!(
        dump("c ? (x) : y;"),
        "expression_statement(expression=conditional(test=c consequent=parenthesized(expression=x) alternate=y))"
    );
    assert_eq!(dump("x as const;"), "expression_statement(expression=as(expression=x type=const))");
    assert_eq!(dump("<T>x;"), "expression_statement(expression=type_assertion(type=type_reference(name=T) expression=x))");
    assert_eq!(
        dump("a?.b!.c?.(d);"),
        "expression_statement(expression=call(callee=member(object=non_null(expression=member(object=a optional=?. property=b)) property=c) optional=?. argument=d))"
    );
}

#[test]
fn jsx_disambiguates_generic_arrows_and_checks_closing_names() {
    let parsed = parse("<T,>(x: T) => x;", true).unwrap();
    assert!(parsed.nodes.iter().any(|node| node.matcher == Kind::Arrow as u32));
    assert!(parse("<a></b>;", true).is_err());
}

#[test]
fn regexes_templates_and_asi() {
    assert_eq!(
        dump("x = a / b / c;"),
        "expression_statement(expression=assignment(left=x operator== right=binary(left=binary(left=a operator=/ right=b) operator=/ right=c)))"
    );
    assert_eq!(
        dump("x = /a\\/b[/]/g.test(s)"),
        "expression_statement(expression=assignment(left=x operator== right=call(callee=member(object=/a\\/b[/]/g property=test) argument=s)))"
    );
    assert_eq!(
        dump("`a${b}c${`d${e}`}`"),
        "expression_statement(expression=template(quasi=`a${ expression=b quasi=}c${ expression=template(quasi=`d${ expression=e quasi=}`) quasi=}`))"
    );
    assert_eq!(
        dump("let a = 1\nlet b = 2"),
        "variable_declaration(kind=let declaration=declarator(name=a init=1)); variable_declaration(kind=let declaration=declarator(name=b init=2))"
    );
}

#[test]
fn declarations_and_types() {
    assert_eq!(
        dump("export default class A<T = string> extends B<T> implements C { private readonly x?: T; get y(): number { return 1 } }"),
        "export(kind=default declaration=class(name=A type_parameter=type_parameter(name=T default=string) extends=B type_argument=type_reference(name=T) implements=type_reference(name=C) member=class_property(modifier=private modifier=readonly key=x optional=? type=type_reference(name=T)) member=method(kind=get key=y return_type=number body=block(body=return(argument=1)))))"
    );
    assert_eq!(
        dump("type F<T> = T extends Promise<infer U> ? U : never;"),
        "type_alias(name=F type_parameter=type_parameter(name=T) type=conditional_type(check=type_reference(name=T) extends=type_reference(name=Promise type_argument=infer(type_parameter=type_parameter(name=U))) consequent=type_reference(name=U) alternate=never))"
    );
    assert_eq!(
        dump("let f: (a: number) => void;"),
        "variable_declaration(kind=let declaration=declarator(name=f type=function_type(parameter=parameter(name=a type=number) return_type=void)))"
    );
    assert_eq!(
        dump("import type { A, type B as C } from \"m\";"),
        "import(kind=type specifier=import_specifier(imported=A) specifier=import_specifier(kind=type imported=B local=C) from=\"m\")"
    );
    assert_eq!(
        dump("function f(x: unknown): x is string;"),
        "function(name=f parameter=parameter(name=x type=unknown) return_type=type_predicate(name=x type=string))"
    );
}

#[test]
fn index_signatures_take_several_parameters() {
    let parsed = parse("type T = { [a: string, b: number]: string };", false).unwrap();
    let signature = parsed.nodes.iter().find(|node| node.matcher == Kind::IndexSig as u32).expect("index signature");
    let parameters = signature.children.iter().filter(|edge| parsed.nodes[edge.node as usize].matcher == Kind::Param as u32).count();
    assert_eq!(parameters, 2);
}

#[test]
fn tokens_rules_point_at() {
    // `catch` and `=>` are where eslint reports; `new X!(a)` keeps its args.
    assert_eq!(dump("try {} catch (e) {}"), "try(block={} kind=catch parameter=e handler={})");
    assert_eq!(
        dump("f(x => x);"),
        "expression_statement(expression=call(callee=f argument=arrow(parameter=parameter(name=x) operator==> body=x)))"
    );
    assert_eq!(dump("new X!(a);"), "expression_statement(expression=new(callee=non_null(expression=X) argument=a))");
}

#[test]
fn every_kind_has_a_unique_layer_name() {
    let mut names: Vec<&str> = ALL_KINDS.iter().map(|k| k.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ALL_KINDS.len());
    assert_eq!(Kind::Program.name(), "ts_program");
}

/// `DXLINT_TS_CORPUS=<dir> cargo test --release corpus -- --ignored --nocapture`
/// parses every TypeScript/JavaScript file under a directory.
#[test]
#[ignore]
fn corpus() {
    let Ok(dir) = std::env::var("DXLINT_TS_CORPUS") else { return };
    let mut failed = 0;
    let mut total = 0;
    let mut bytes = 0;
    let mut slow = Vec::new();
    let started = std::time::Instant::now();
    for entry in ignore::WalkBuilder::new(&dir).build().flatten() {
        let path = entry.path();
        let Some(jsx) = super::reads(path) else { continue };
        let Ok(src) = std::fs::read_to_string(path) else { continue };
        total += 1;
        bytes += src.len();
        let t = std::time::Instant::now();
        let r = parse(&src, jsx);
        slow.push((t.elapsed(), path.display().to_string()));
        if let Err((at, e)) = r {
            failed += 1;
            let line = src[..at as usize].matches('\n').count() + 1;
            if failed <= 40 {
                println!("{}:{line}: {e}", path.display());
            }
        }
    }
    slow.sort();
    for (t, p) in slow.iter().rev().take(5) {
        println!("slow: {t:?} {p}");
    }
    println!("{failed} of {total} files failed, {} MiB in {:?}", bytes >> 20, started.elapsed());
}
