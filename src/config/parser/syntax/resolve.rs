//! Forward token references resolve inside their declared parser namespace.

use crate::pattern::syntax::{Definition, Expr, TokenMatch};

pub(super) fn resolve(definition: &mut Definition) -> Result<(), String> {
    let tokens: Vec<_> = definition.tokens.iter().map(|token| token.name.clone()).collect();
    let kinds: Vec<_> = definition.nodes.iter().map(|node| node.name.clone()).collect();
    for production in &mut definition.productions {
        expression(&mut production.body, &tokens)?;
        if kinds.contains(&production.name)
            && !matches!(&production.body, Expr::Emit { kind, .. } if kind == &production.name)
        {
            production.body = Expr::Emit {
                kind: production.name.clone(),
                inner: Box::new(std::mem::replace(&mut production.body, Expr::Seq(Vec::new()))),
            };
        }
    }
    Ok(())
}

fn expression(body: &mut Expr, tokens: &[String]) -> Result<(), String> {
    match body {
        Expr::Call { production, min_binding_power } if tokens.contains(production) => {
            if *min_binding_power != 0 { return Err("a token cannot take binding power".into()); }
            *body = Expr::Token(TokenMatch { token: production.clone(), text: None, emit: None });
        }
        Expr::Seq(items) | Expr::Choice(items) => for item in items { expression(item, tokens)?; },
        Expr::Repeat { inner, .. } | Expr::Bind { inner, .. } | Expr::Emit { inner, .. } |
        Expr::Context { inner, .. } | Expr::Look { inner, .. } | Expr::Try(inner) | Expr::Discard(inner) | Expr::Text(inner) => expression(inner, tokens)?,
        Expr::Flag { yes, no, .. } => {
            expression(yes, tokens)?;
            expression(no, tokens)?;
        }
        Expr::Scope { header, body, .. } => {
            expression(header, tokens)?;
            expression(body, tokens)?;
        }
        Expr::Pratt(precedence) => {
            expression(&mut precedence.atom, tokens)?;
            for prefix in &mut precedence.prefix { expression(&mut prefix.operator, tokens)?; }
            for row in &mut precedence.rows { expression(&mut row.operator, tokens)?; }
        }
        _ => {}
    }
    Ok(())
}
