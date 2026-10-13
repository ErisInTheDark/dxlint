use super::{IndexedMode, Key, Known, Leaf, Member, MemberOrigin, MemberSource, Values};
use crate::config::ast::values::KeyDomain;

#[derive(Clone, Copy)]
pub(super) enum MemberRequest<'a> {
    Listing,
    Coverage,
    Key(&'a Key),
}

impl<'a> MemberRequest<'a> {
    pub(super) fn key(self) -> Option<&'a Key> {
        match self {
            Self::Key(key) => Some(key),
            Self::Listing | Self::Coverage => None,
        }
    }
}

impl Values<'_> {
    pub(super) fn indexed_members(&self, at: Leaf, request: MemberRequest<'_>, named: bool) -> Known<Vec<Member>> {
        let wanted = request.key();
        let Known::Yes((node, view)) = self.member_context(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut members = Vec::new();
        let classes = view.map_or(kind.member_defaults, |(_, classes)| classes);
        for recipe in &kind.indexed {
            if matches!(recipe.mode, IndexedMode::Keyed(_)) != named { continue }
            if recipe.views != 0 && recipe.views & classes == 0 { continue }
            let wanted_position = match &recipe.mode {
                IndexedMode::Slots { repeated, strings } => match wanted {
                    Some(Key::Ordinal(position)) => Some(*position),
                    Some(Key::Domain(KeyDomain::Ordinal)) if *repeated => None,
                    Some(key @ Key::Text(_)) if *strings => match key.ordinal() {
                        Some(position) => Some(position),
                        _ => continue,
                    },
                    Some(_) => continue,
                    None => None,
                },
                IndexedMode::Domain(domain) => {
                    if wanted.is_some_and(|key| !Self::domain_accepts(*domain, key)) { continue }
                    None
                },
                IndexedMode::Keyed(_) => None,
            };
            if !self.question.budget.step() { return Known::Unknown }
            match recipe.when.map_or(Known::Yes(true), |when| self.world.condition(at, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let Known::Yes(sites) = self.follow(at, &recipe.path) else { return Known::Unknown };
            let (repeated, strings) = match &recipe.mode {
                IndexedMode::Slots { repeated, strings } => (*repeated, *strings),
                IndexedMode::Domain(domain) => {
                    if sites.is_empty() { continue }
                    if matches!(request, MemberRequest::Coverage) { continue }
                    let Some(key) = wanted else { return Known::Unknown };
                    Self::merge_member(&mut members, Member {
                        key: key.clone(),
                        sources: sites.into_iter().map(|site| MemberSource {
                            site, value: recipe.value.clone(), origin: MemberOrigin::Indexer(*domain),
                        }).collect(),
                        required: Known::Unknown,
                    });
                    continue;
                },
                IndexedMode::Keyed(expression) => {
                    for site in sites {
                        if !self.question.budget.step() { return Known::Unknown }
                        let Known::Yes(key) = self.key(site, expression) else { return Known::Unknown };
                        if matches!(key, Key::Domain(_)) { return Known::Unknown }
                        if wanted.is_some_and(|wanted| wanted != &key) { continue }
                        let required = match self.optional(site) {
                            Known::Yes(optional) => Known::Yes(!optional),
                            Known::Unknown => Known::Unknown,
                        };
                        Self::merge_member(&mut members, Member {
                            key, sources: vec![MemberSource { site, value: recipe.value.clone(), origin: MemberOrigin::Named }], required,
                        });
                    }
                    continue;
                },
            };
            if repeated {
                if sites.len() != 1 { return Known::Unknown }
                if matches!(request, MemberRequest::Coverage) { continue }
                let Some(key) = wanted.cloned() else { return Known::Unknown };
                Self::merge_member(&mut members, Member {
                    key,
                    sources: sites.into_iter().map(|site| MemberSource {
                        site, value: recipe.value.clone(), origin: MemberOrigin::Ordinal { repeated: true },
                    }).collect(),
                    required: Known::Unknown,
                });
                continue;
            }
            for (position, site) in sites.into_iter().enumerate() {
                if !self.question.budget.step() { return Known::Unknown }
                let Ok(position) = u32::try_from(position) else { return Known::Unknown };
                if wanted_position.is_some_and(|wanted| wanted != position) { continue }
                let key = wanted.cloned().unwrap_or(Key::Ordinal(position));
                let required = match self.optional(site) {
                    Known::Yes(optional) => Known::Yes(!optional),
                    Known::Unknown => Known::Unknown,
                };
                let source = MemberSource { site, value: recipe.value.clone(), origin: MemberOrigin::Ordinal { repeated: false } };
                Self::merge_member(&mut members, Member { key, sources: vec![source.clone()], required });
                if wanted.is_none() && strings {
                    Self::merge_member(&mut members, Member {
                        key: Key::Text(position.to_string().into()), sources: vec![source], required,
                    });
                }
            }
        }
        Known::Yes(members)
    }

    fn domain_accepts(domain: KeyDomain, key: &Key) -> bool {
        match (domain, key) {
            (KeyDomain::Text, Key::Text(_) | Key::Ordinal(_) | Key::Domain(KeyDomain::Text | KeyDomain::Ordinal))
            | (KeyDomain::Ordinal, Key::Ordinal(_) | Key::Domain(KeyDomain::Ordinal))
            | (KeyDomain::Symbol, Key::Symbol(_) | Key::Domain(KeyDomain::Symbol)) => true,
            (KeyDomain::Ordinal, key @ Key::Text(_)) => key.ordinal().is_some(),
            _ => false,
        }
    }
}
