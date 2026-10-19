use super::{IndexedMode, Key, Known, Leaf, Member, MemberOrigin, MemberSource, NodeValue, Target, Values};
use crate::config::ast::values::KeyDomain;

#[derive(Clone, Copy)]
pub(super) enum MemberRequest<'a> {
    /// Every member, by concrete key; unknown with an indexer.
    Listing,
    /// Every member, with indexers by their key domain (structural shape).
    Shape,
    Coverage,
    Key(&'a Key),
}

impl<'a> MemberRequest<'a> {
    pub(super) fn key(self) -> Option<&'a Key> {
        match self {
            Self::Key(key) => Some(key),
            Self::Listing | Self::Shape | Self::Coverage => None,
        }
    }

    /// The key and requirement an indexer over `domain` lists under:
    /// `Shape` names it by its domain (no key required); a plain listing
    /// cannot enumerate it.
    fn indexer(self, domain: KeyDomain) -> Option<(Key, Known<bool>)> {
        match self {
            Self::Key(key) => Some((key.clone(), Known::Unknown)),
            Self::Shape => Some((Key::Domain(domain), Known::Yes(false))),
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
            if matches!(recipe.mode, IndexedMode::Keyed(_) | IndexedMode::Keys { .. }) != named { continue }
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
                IndexedMode::Keyed(_) | IndexedMode::Keys { .. } => None,
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
                    let Some((key, required)) = request.indexer(*domain) else { return Known::Unknown };
                    Self::merge_member(&mut members, Member {
                        key,
                        sources: sites.into_iter().map(|site| MemberSource {
                            site, value: recipe.value.clone(), origin: MemberOrigin::Indexer(*domain), edits: None,
                        }).collect(),
                        required,
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
                            key, sources: vec![MemberSource { site, value: recipe.value.clone(), origin: MemberOrigin::Named, edits: None }], required,
                        });
                    }
                    continue;
                },
                IndexedMode::Keys { bind } => {
                    let Known::Yes(found) = self.keys_members(at, request, recipe.value.as_ref(), bind.as_deref(), sites) else {
                        return Known::Unknown;
                    };
                    for member in found { Self::merge_member(&mut members, member) }
                    continue;
                },
            };
            if repeated {
                if sites.len() != 1 { return Known::Unknown }
                if matches!(request, MemberRequest::Coverage) { continue }
                let Some((key, required)) = request.indexer(KeyDomain::Ordinal) else { return Known::Unknown };
                Self::merge_member(&mut members, Member {
                    key,
                    sources: sites.into_iter().map(|site| MemberSource {
                        site, value: recipe.value.clone(), origin: MemberOrigin::Ordinal { repeated: true }, edits: None,
                    }).collect(),
                    required,
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
                let source = MemberSource { site, value: recipe.value.clone(), origin: MemberOrigin::Ordinal { repeated: false }, edits: None };
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

    /// `indexed keys`: one member per key the sites' values hold, an indexer
    /// per key domain; each member's value is read at `at` with formal `bind`
    /// bound to the alternatives giving its key.
    fn keys_members(&self, at: Leaf, request: MemberRequest<'_>, value: Option<&super::Value>, bind: Option<&[u16]>, sites: Vec<Leaf>) -> Known<Vec<Member>> {
        let wanted = request.key();
        // Members are read at the owner node itself (its optional/readonly
        // metadata), not at a scope standing for it.
        let Known::Yes((owner, _)) = self.member_context(at) else { return Known::Unknown };
        let at = Leaf { target: Target::Node(owner), ..at };
        let binding = match bind {
            Some(bind) => {
                let Known::Yes(formals) = self.follow(at, bind) else { return Known::Unknown };
                let [formal] = formals.as_slice() else { return Known::Unknown };
                let Known::Yes((owner, position, count)) = self.parameter_owner(at, *formal) else { return Known::Unknown };
                let Known::Yes(classes) = self.question.owner_classes(at.frame, owner) else { return Known::Unknown };
                Some((owner, position, count, classes))
            }
            None => None,
        };
        let mut members = Vec::new();
        for site in sites {
            if !self.question.budget.step() { return Known::Unknown }
            let keys = self.values(site);
            // No key at all (`never`) generates no members.
            if keys == NodeValue::never() { continue }
            let Known::Yes(keys) = self.value_key_rows(&keys) else { return Known::Unknown };
            for (key, rows) in keys {
                let origin = match &key {
                    Key::Domain(domain) => {
                        if wanted.is_some_and(|wanted| !Self::domain_accepts(*domain, wanted)) { continue }
                        if matches!(request, MemberRequest::Coverage) { continue }
                        MemberOrigin::Indexer(*domain)
                    }
                    key => {
                        if wanted.is_some_and(|wanted| wanted != key) { continue }
                        MemberOrigin::Named
                    }
                };
                let source = match binding {
                    Some((owner, position, count, classes)) => {
                        let mut arguments = vec![None; count];
                        arguments[position] = Some(NodeValue::rows(rows));
                        Leaf { frame: self.question.bind_overlay(at.frame, owner, arguments, classes), ..at }
                    }
                    None => at,
                };
                let (key, required) = match &origin {
                    MemberOrigin::Indexer(domain) => {
                        let Some(indexer) = request.indexer(*domain) else { return Known::Unknown };
                        indexer
                    }
                    _ => {
                        let required = match self.optional(source) {
                            Known::Yes(optional) => Known::Yes(!optional),
                            Known::Unknown => Known::Unknown,
                        };
                        (key, required)
                    }
                };
                Self::merge_member(&mut members, Member {
                    key, sources: vec![MemberSource { site: source, value: value.cloned(), origin, edits: None }], required,
                });
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
