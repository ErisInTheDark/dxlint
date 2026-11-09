//! `dxlint --profile`: the engine's tables and reports over the counters
//! [`dx_core::profile`] keeps.

use std::path::PathBuf;

pub use dx_core::profile::*;

/// A profile sized for the engine's matchers, rules, facts and semantic
/// query names.
pub fn new_profile(matchers: usize, rules: usize) -> Profile {
    Profile::new(matchers, rules, crate::expr::facts::FACTS.len(), semantic_queries())
}

/// Every semantic query name the engine records.
fn semantic_queries() -> impl Iterator<Item = &'static str> {
    crate::expr::queries::QUERY_FUNCTIONS.iter().flat_map(|group| group.iter())
        .map(|query| query.name).chain(crate::expr::queries::QUERIES.iter().map(|query| query.name))
        .chain(crate::grammar::names::calls::ProfileQuery::ALL.iter().map(|query| query.name()))
        .chain(crate::grammar::names::modules::queries::PROFILE_QUERIES.iter().copied())
        .chain(["units.native", "units.view", "modules.resolve.search", "modules.resolve.shared", "modules.resolve.shared.key", "modules.resolve.shared.select", "modules.resolve.shared.identity","modules.resolve.shared.lookup", "modules.resolve.shared.targets", "modules.resolve.shared.record", "modules.resolve.shared.publish", "modules.resolve.catalog", "modules.roots.inputs", "modules.roots.survey", "modules.roots.candidates", "modules.roots.prefetch", "modules.roots.attach", "modules.identity.inputs", "modules.identity.scan"])
}

/// One line on the busiest semantic queries so far (by self time), for
/// watching a run that never finishes (`DXLINT_PROFILE_EVERY`).
/// `entered/finished` counts show what a stuck run keeps starting;
/// `attribute` names a `let` by index.
pub fn watch_line(profile: &Profile, attribute: impl Fn(usize) -> String) -> String {
    let mut queries: Vec<_> = profile.semantic_queries.iter()
        .map(|(&name, query)| (Profile::get(&query.entered), Profile::get(&query.calls), Profile::get(&query.self_nanos), name))
        .filter(|&(entered, ..)| entered > 0).collect();
    queries.sort_by_key(|row| std::cmp::Reverse(row.0));
    let queries: Vec<String> = queries.iter().take(8)
        .map(|(entered, calls, nanos, name)| format!("{name} {entered}/{calls} {:.0}ms", *nanos as f64 / 1e6)).collect();
    let mut lets: Vec<_> = profile.attr_entered.iter().enumerate()
        .map(|(index, entered)| (Profile::get(entered), Profile::get(&profile.attr_calls[index]), index))
        .filter(|&(entered, ..)| entered > 0).collect();
    lets.sort_by_key(|row| std::cmp::Reverse(row.0));
    let lets: Vec<String> = lets.iter().take(6)
        .map(|&(entered, calls, index)| format!("{} {entered}/{calls}", attribute(index))).collect();
    format!("dxlint profile: queries {} | lets {}", queries.join(", "), lets.join(", "))
}

pub fn slowest_files(profile: &Profile) -> Vec<(u64, String)> {
    profile.slowest_files.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `(steps on foreign nodes, nodes, nodes computed more than once, steps
/// those repeats spent, steps repeated by other linted files)`: what
/// sharing foreign values across files can save (the last excludes a
/// file's own repeats under other frames).
pub fn foreign_summary(profile: &Profile) -> (u64, usize, usize, u64, u64) {
    let nodes = profile.foreign_nodes.lock().expect("foreign nodes lock");
    let repeated = nodes.values().filter(|node| node.computations > 1);
    let repeat_steps = repeated.clone().map(|node| node.steps - node.steps / node.computations).sum();
    let cross_steps = nodes.values().map(|node| {
        let files = node.linted.len() as u64;
        if files > 1 { node.steps - node.steps / files } else { 0 }
    }).sum();
    (Profile::get(&profile.foreign_steps), nodes.len(), repeated.count(), repeat_steps, cross_steps)
}

/// Cut root questions by query and code, most first.
pub fn cut_roots(profile: &Profile) -> Vec<(u64, &'static str, Box<str>)> {
    let mut rows: Vec<_> = profile.cut_roots.lock().expect("cut roots lock").iter()
        .map(|((query, text), &count)| (count, *query, text.clone())).collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    rows
}

pub fn effect_failures(profile: &Profile) -> Vec<(PathBuf, u32, Failure, EffectFailureProfile)> {
    let failures = profile.effect_failures.lock().expect("effect profiling lock");
    let mut rows: Vec<_> = failures.iter().map(|((path, node, cause), row)| (path.clone(), *node, *cause, *row)).collect();
    rows.sort_by_key(|(_, _, _, row)| std::cmp::Reverse(row.nanos));
    rows
}

#[cfg(test)]
mod tests {
    use super::Profile;

    #[test]
    fn trait_queries_are_counted_when_linting() {
        let source = r#"
            lang plain "*.t"
            in .plain match item /[a-z]/
            in item fits marked
            in item rule probe info text { when node could marked && node always marked }
        "#;
        let built = crate::engine::build(
            crate::config::load_text("profile.dx", source, &crate::config::source::Disk),
            std::path::PathBuf::from("/root"),
        );
        assert!(built.errors.is_empty(), "{:?}", built.errors);
        let mut linter = built.linter.expect("valid grammar");
        linter.enable_profile();
        assert_eq!(linter.lint(std::path::Path::new("/root/file.t"), "x").len(), 1);
        let profile = linter.profile.as_ref().unwrap();
        for query in crate::expr::queries::QUERIES {
            assert!(Profile::get(&profile.semantic_queries[query.name].calls) > 0, "{}", query.name);
        }
    }
}
