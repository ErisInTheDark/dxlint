//! JSDoc comments: the `/** ... */` right before a declaration.

/// The JSDoc comment directly before `start` (whitespace between allowed),
/// without its `/**`, `*/` and line-leading `*`s.
pub fn leading_doc(src: &str, start: usize) -> Option<String> {
    let before = src[..start].trim_end();
    let body_end = before.strip_suffix("*/")?.len();
    let open = before[..body_end].rfind("/**")?;
    // `/**/` and comments that close earlier are not this one's doc.
    if before[open + 3..body_end].contains("*/") {
        return None;
    }
    let body = &before[open + 3..body_end];
    let lines: Vec<&str> = body
        .lines()
        .map(|l| {
            let l = l.trim();
            let l = l.strip_prefix('*').unwrap_or(l);
            l.strip_prefix(' ').unwrap_or(l).trim_end()
        })
        .collect();
    Some(lines.join("\n").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::leading_doc;

    #[test]
    fn docs_lose_their_frame() {
        let src = "/**\n * Old.\n * @deprecated Use {@link b}\n */\nexport function a() {}";
        let at = src.find("export").unwrap();
        assert_eq!(leading_doc(src, at).as_deref(), Some("Old.\n@deprecated Use {@link b}"));
        assert_eq!(leading_doc("/* no */ x", 9), None);
        assert_eq!(leading_doc("/** a */ /* b */ x", 17), None);
    }
}
