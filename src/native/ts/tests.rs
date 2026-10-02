use super::kinds::{Kind, ALL_KINDS, FIELD_NAMES};
use super::parse;
use crate::grammar::TEXT;

/// `kind(field=child ...)` with leaves as their text, for compact asserts.
fn dump(src: &str) -> String {
    let p = parse(src, false).unwrap_or_else(|(at, e)| panic!("{e} at {at}: {}", &src[at as usize..]));
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
    assert_eq!(dump("a || b && c;"), "expr_stmt(expr=logical(left=a op=|| right=logical(left=b op=&& right=c)))");
    assert_eq!(dump("a ** b ** c;"), "expr_stmt(expr=binary(left=a op=** right=binary(left=b op=** right=c)))");
    assert_eq!(dump("x = a >> b >= c;"), "expr_stmt(expr=assign(left=x op== right=binary(left=binary(left=a op=>> right=b) op=>= right=c)))");
    assert_eq!(dump("x >>>= 1;"), "expr_stmt(expr=assign(left=x op=>>>= right=1))");
}

#[test]
fn arrows_generics_and_casts() {
    assert_eq!(dump("f<T>(x);"), "expr_stmt(expr=call(callee=f type_arg=type_ref(name=T) arg=x))");
    assert_eq!(dump("a < b > c;"), "expr_stmt(expr=binary(left=binary(left=a op=< right=b) op=> right=c))");
    assert_eq!(dump("(a, b): T => a;"), "expr_stmt(expr=arrow(param=param(name=a) param=param(name=b) return_type=type_ref(name=T) body=a))");
    assert_eq!(dump("async x => x;"), "expr_stmt(expr=arrow(modifier=async param=param(name=x) body=x))");
    assert_eq!(dump("c ? (x) : y;"), "expr_stmt(expr=conditional(test=c consequent=paren(expr=x) alternate=y))");
    assert_eq!(dump("x as const;"), "expr_stmt(expr=as(expr=x type=const))");
    assert_eq!(dump("<T>x;"), "expr_stmt(expr=type_assertion(type=type_ref(name=T) expr=x))");
    assert_eq!(dump("a?.b!.c?.(d);"), "expr_stmt(expr=call(callee=member(object=non_null(expr=member(object=a optional=?. property=b)) property=c) optional=?. arg=d))");
}

#[test]
fn regexes_templates_and_asi() {
    assert_eq!(dump("x = a / b / c;"), "expr_stmt(expr=assign(left=x op== right=binary(left=binary(left=a op=/ right=b) op=/ right=c)))");
    assert_eq!(dump("x = /a\\/b[/]/g.test(s)"), "expr_stmt(expr=assign(left=x op== right=call(callee=member(object=/a\\/b[/]/g property=test) arg=s)))");
    assert_eq!(dump("`a${b}c${`d${e}`}`"), "expr_stmt(expr=template(quasi=`a${ expr=b quasi=}c${ expr=template(quasi=`d${ expr=e quasi=}`) quasi=}`))");
    assert_eq!(dump("let a = 1\nlet b = 2"), "var(kind=let decl=declarator(name=a init=1)); var(kind=let decl=declarator(name=b init=2))");
}

#[test]
fn declarations_and_types() {
    assert_eq!(
        dump("export default class A<T = string> extends B<T> implements C { private readonly x?: T; get y(): number { return 1 } }"),
        "export(kind=default decl=class(name=A type_param=type_param(name=T default=string) extends=B type_arg=type_ref(name=T) implements=type_ref(name=C) member=class_prop(modifier=private modifier=readonly key=x optional=? type=type_ref(name=T)) member=method(kind=get key=y return_type=number body=block(body=return(arg=1)))))"
    );
    assert_eq!(
        dump("type F<T> = T extends Promise<infer U> ? U : never;"),
        "type_alias(name=F type_param=type_param(name=T) type=conditional_type(check=type_ref(name=T) extends=type_ref(name=Promise type_arg=infer(type_param=type_param(name=U))) consequent=type_ref(name=U) alternate=never))"
    );
    assert_eq!(dump("let f: (a: number) => void;"), "var(kind=let decl=declarator(name=f type=fn_type(param=param(name=a type=number) return_type=void)))");
    assert_eq!(dump("import type { A, type B as C } from \"m\";"), "import(kind=type specifier=import_spec(imported=A) specifier=import_spec(kind=type imported=B local=C) from=\"m\")");
    assert_eq!(dump("function f(x: unknown): x is string;"), "function(name=f param=param(name=x type=unknown) return_type=type_predicate(name=x type=string))");
}

#[test]
fn every_kind_has_a_unique_layer_name() {
    let mut names: Vec<&str> = ALL_KINDS.iter().map(|k| k.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ALL_KINDS.len());
    assert_eq!(Kind::Program.name(), "ts_program");
}

/// `ZLINT_TS_CORPUS=<dir> cargo test --release corpus -- --ignored --nocapture`
/// parses every TypeScript/JavaScript file under a directory.
#[test]
#[ignore]
fn corpus() {
    let Ok(dir) = std::env::var("ZLINT_TS_CORPUS") else { return };
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
