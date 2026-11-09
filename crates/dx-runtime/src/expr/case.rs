//! Identifier case conversion. Word splitting understands `snake_case`,
//! `kebab-case`, `camelCase`, `PascalCase`, `SCREAMING_CASE` and acronyms
//! like `HTTPServer`.

pub fn words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    for chunk in s.split(|c: char| !c.is_alphanumeric()).filter(|c| !c.is_empty()) {
        let chars: Vec<char> = chunk.chars().collect();
        let mut current = String::new();
        for (i, &c) in chars.iter().enumerate() {
            let prev = i.checked_sub(1).map(|p| chars[p]);
            let next = chars.get(i + 1).copied();
            let boundary = match prev {
                Some(p) if c.is_uppercase() => {
                    p.is_lowercase() || p.is_ascii_digit() || (p.is_uppercase() && next.is_some_and(char::is_lowercase))
                }
                _ => false,
            };
            if boundary && !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        if !current.is_empty() {
            words.push(current);
        }
    }
    words
}

/// Keeps leading/trailing `_` (privacy markers like `_unused`).
fn with_affixes(s: &str, body: String) -> String {
    let lead = s.len() - s.trim_start_matches('_').len();
    let trail = if lead == s.len() { 0 } else { s.len() - s.trim_end_matches('_').len() };
    format!("{}{}{}", &s[..lead], body, &s[s.len() - trail..])
}

fn capitalize(w: &str) -> String {
    let mut chars = w.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

pub fn snake(s: &str) -> String {
    with_affixes(s, words(s).iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"))
}

pub fn screaming(s: &str) -> String {
    with_affixes(s, words(s).iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"))
}

pub fn kebab(s: &str) -> String {
    words(s).iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("-")
}

pub fn pascal(s: &str) -> String {
    with_affixes(s, words(s).iter().map(|w| capitalize(w)).collect())
}

pub fn camel(s: &str) -> String {
    let ws = words(s);
    let body: String = ws
        .iter()
        .enumerate()
        .map(|(i, w)| if i == 0 { w.to_lowercase() } else { capitalize(w) })
        .collect();
    with_affixes(s, body)
}

pub fn title(s: &str) -> String {
    words(s).iter().map(|w| capitalize(w)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_acronyms_and_digits() {
        assert_eq!(words("parseHTTPServer2Fast"), vec!["parse", "HTTP", "Server2", "Fast"]);
        assert_eq!(words("already_snake-kebab"), vec!["already", "snake", "kebab"]);
    }

    #[test]
    fn conversions_round_trip_and_keep_affixes() {
        assert_eq!(snake("doHTTPThing"), "do_http_thing");
        assert_eq!(snake("_privateField"), "_private_field");
        assert_eq!(camel("make_it_so"), "makeItSo");
        assert_eq!(pascal("make-it-so"), "MakeItSo");
        assert_eq!(screaming("maxValue"), "MAX_VALUE");
        assert_eq!(kebab("MaxValue"), "max-value");
        assert_eq!(title("max_value"), "Max Value");
    }
}
