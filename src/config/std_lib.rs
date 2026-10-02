//! The standard library, embedded in the binary: `import "std/rust"`.

static FILES: &[(&str, &str)] = &[
    ("core", include_str!("../../std/core.zlint")),
    ("rust", include_str!("../../std/rust.zlint")),
    ("js", include_str!("../../std/js.zlint")),
    ("ts", include_str!("../../std/ts.zlint")),
    ("python", include_str!("../../std/python.zlint")),
    ("c", include_str!("../../std/c.zlint")),
    ("zlint", include_str!("../../std/zlint.zlint")),
    ("markdown", include_str!("../../std/markdown.zlint")),
    ("toml", include_str!("../../std/toml.zlint")),
    ("whitespace", include_str!("../../std/whitespace.zlint")),
];

pub fn get(name: &str) -> Option<&'static str> {
    let name = name.strip_suffix(".zlint").unwrap_or(name);
    FILES.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(n, _)| *n)
}
