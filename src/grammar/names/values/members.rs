use super::{eval, Key, Known, Leaf, Member, MemberSource, NodeRef, NodeValue, Target, Values};
use super::indexed::MemberRequest;

impl Values<'_> {
    pub fn member_shape(&self, at: Leaf) -> Known<bool> {
        let at = self.modified_source(at).unwrap_or(at);
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        if matches!(at.target, Target::Product(_)) {
            return match self.product_info(at) {
                Known::Yes(Some(_)) => Known::Yes(true),
                _ => Known::Unknown,
            };
        }
        if matches!(at.target, Target::Traits(_)) { return Known::Yes(false) }
        if let Target::Scope(scope) = at.target {
            match self.semantic.modules().publication_names(scope) {
                Known::Yes(Some(_)) => return Known::Yes(true),
                Known::Unknown => return Known::Unknown,
                Known::Yes(None) => {}
            }
        }
        let Known::Yes((node, view)) = self.member_context(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let classes = view.map_or(kind.member_defaults, |(_, classes)| classes);
        if classes & kind.member_defaults != 0 { return Known::Yes(true) }
        let declarations = kind.indexed.iter().map(|indexed| (indexed.views, indexed.when))
            .chain(kind.member_bases.iter().map(|(views, _, when)| (*views, *when)))
            .chain(kind.member_links.iter().map(|(views, _, _, when)| (*views, *when)));
        let mut unknown = false;
        for (views, when) in declarations {
            if views != 0 && views & classes == 0 { continue }
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn has_member(&self, value: &NodeValue, key: &Key) -> Known<bool> {
        let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
        let mut unknown = false;
        for row in rows {
            if row.is_empty() { unknown = true; }
            for &leaf in row {
                if !self.question.budget.step() { return Known::Unknown }
                match self.leaf_member(leaf, key) {
                    Known::Yes(Some(member)) if member.sources.iter().any(|source| source.origin == super::MemberOrigin::Named) => {
                        return Known::Yes(true);
                    },
                    Known::Yes(None) if self.closed_leaf(leaf) == Known::Yes(true) => {},
                    _ => unknown = true,
                }
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn closed(&self, value: &NodeValue) -> Known<bool> {
        let NodeValue::Alternatives(rows) = value else { return Known::Unknown };
        let mut unknown = false;
        for row in rows {
            if row.is_empty() { return Known::Yes(false) }
            for &leaf in row {
                if !self.question.budget.step() { return Known::Unknown }
                match self.closed_leaf(leaf) {
                    Known::Yes(false) => return Known::Yes(false),
                    Known::Unknown => unknown = true,
                    Known::Yes(true) => {}
                }
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(true) }
    }

    fn closed_leaf(&self, at: Leaf) -> Known<bool> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.memoized(&self.semantic.values.member_closed, at, || self.direct_closed(at))
    }

    fn direct_closed(&self, at: Leaf) -> Known<bool> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        if let Some(source) = self.modified_source(at) { return self.closed_leaf(source) }
        if matches!(at.target, Target::Product(_)) {
            return match self.product_members(at, None) {
                Known::Yes(_) => Known::Yes(true),
                Known::Unknown => Known::Unknown,
            };
        }
        match self.apparent(at) {
            Known::Yes(Some(inherited)) => return self.closed(&inherited),
            Known::Yes(None) => {}
            Known::Unknown => return Known::Unknown,
        }
        if matches!(at.target, Target::Traits(_)) { return Known::Yes(false) }
        if let Target::Scope(scope) = at.target {
            match self.semantic.modules().publication_names(scope) {
                Known::Yes(Some(_)) => return match self.leaf_members(at) {
                    Known::Yes(_) => Known::Yes(true),
                    Known::Unknown => Known::Unknown,
                },
                Known::Unknown => return Known::Unknown,
                Known::Yes(None) => {}
            }
        }
        let Known::Yes((node, view)) = self.member_context(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(syntax) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(syntax.matcher as usize) else { return Known::Unknown };
        let classes = view.map_or(kind.member_defaults, |(_, classes)| classes);
        if classes == 0 { return Known::Yes(false) }
        let mut closed = 0;
        let mut uncertain = 0;
        for &(views, when) in &kind.member_closed {
            if classes & views == 0 { continue }
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(true) => closed |= views,
                Known::Unknown => uncertain |= views,
                Known::Yes(false) => {}
            }
        }
        if classes & closed != classes {
            return if classes & (closed | uncertain) == classes { Known::Unknown } else { Known::Yes(false) };
        }
        let mut unknown = false;
        for (views, base, when) in &kind.member_bases {
            if classes & views == 0 { continue }
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => { unknown = true; continue }
                Known::Yes(true) => {}
            }
            match self.closed(&self.evaluate(at, base)) {
                Known::Yes(false) => return Known::Yes(false),
                Known::Unknown => unknown = true,
                Known::Yes(true) => {}
            }
        }
        if unknown || matches!(self.direct_members(at, MemberRequest::Coverage), Known::Unknown) { Known::Unknown } else { Known::Yes(true) }
    }

    pub fn member(&self, value: &NodeValue, key: &Key) -> NodeValue {
        let Some(_nested) = self.question.budget.nest() else { return NodeValue::Unknown };
        let Known::Yes(rows) = self.keyed_members(value, key) else { return NodeValue::Unknown };
        let mut alternatives = Vec::new();
        for row in rows {
            let Some(member) = row else { return NodeValue::Unknown };
            if member.sources.is_empty() { return NodeValue::Unknown }
            alternatives.push(eval::combine(self, member.sources.into_iter().map(|source| {
                source.value.as_ref().map_or_else(|| self.values(source.site), |value| self.evaluate(source.site, value))
            }), true));
        }
        eval::combine(self, alternatives, false)
    }

    pub fn member_sites(&self, value: &NodeValue, key: &Key) -> Known<Vec<Leaf>> {
        let Known::Yes(rows) = self.keyed_members(value, key) else { return Known::Unknown };
        if rows.iter().all(Option::is_none) {
            return if self.closed(value) == Known::Yes(true) { Known::Yes(Vec::new()) } else { Known::Unknown };
        }
        let mut sites = Vec::new();
        for row in rows {
            let Some(member) = row else { return Known::Unknown };
            for source in member.sources {
                let site = source.site;
                if !self.question.budget.step() { return Known::Unknown }
                if !sites.contains(&site) { sites.push(site) }
            }
        }
        Known::Yes(sites)
    }

    pub fn members(&self, value: &NodeValue) -> Known<Vec<Vec<Member>>> {
        self.member_rows(value, false)
    }

    /// Like [`Values::members`], but indexers are listed once by their key
    /// domain, for structural comparison of open shapes (arrays, records).
    pub fn shape_members(&self, value: &NodeValue) -> Known<Vec<Vec<Member>>> {
        self.member_rows(value, true)
    }

    fn member_rows(&self, value: &NodeValue, shape: bool) -> Known<Vec<Vec<Member>>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let NodeValue::Alternatives(alternatives) = value else { return Known::Unknown };
        let mut rows = Vec::new();
        for alternative in alternatives {
            if alternative.is_empty() { return Known::Unknown }
            let mut row = Vec::new();
            for &leaf in alternative {
                if !self.question.budget.step() { return Known::Unknown }
                let members = if shape {
                    let Known::Yes(at) = self.semantic.leaf(leaf) else { return Known::Unknown };
                    self.direct_members(at, MemberRequest::Shape)
                } else { self.leaf_members(leaf) };
                let Known::Yes(members) = members else { return Known::Unknown };
                for member in members { Self::merge_member(&mut row, member) }
            }
            rows.push(row);
        }
        Known::Yes(rows)
    }

    pub fn keyed_members(&self, value: &NodeValue, key: &Key) -> Known<Vec<Option<Member>>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let NodeValue::Alternatives(alternatives) = value else { return Known::Unknown };
        let mut rows = Vec::new();
        for alternative in alternatives {
            if alternative.is_empty() { return Known::Unknown }
            let mut row = Vec::new();
            let mut unknown = false;
            for &leaf in alternative {
                if !self.question.budget.step() { return Known::Unknown }
                match self.leaf_member(leaf, key) {
                    Known::Yes(Some(member)) => Self::merge_member(&mut row, member),
                    Known::Yes(None) => {}
                    Known::Unknown => unknown = true,
                }
            }
            // An alternative's leaves all hold at once: a member one of them
            // is known to have is there even when another leaf is opaque.
            // Its absence needs every leaf known.
            if unknown && row.is_empty() { return Known::Unknown }
            rows.push(row.pop());
        }
        Known::Yes(rows)
    }

    fn leaf_member(&self, at: Leaf, key: &Key) -> Known<Option<Member>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.memoized(&self.semantic.values.member_keys, (at, key.clone()), || match self.direct_members(at, MemberRequest::Key(key)) {
            Known::Yes(mut members) => Known::Yes(members.pop()),
            Known::Unknown => Known::Unknown,
        })
    }

    pub(super) fn merge_member(members: &mut Vec<Member>, member: Member) {
        match members.iter_mut().find(|existing| existing.key == member.key) {
            Some(existing) => {
                let existing_named = existing.sources.iter().any(|source| source.origin == super::MemberOrigin::Named);
                let incoming_named = member.sources.iter().any(|source| source.origin == super::MemberOrigin::Named);
                let only_indexers = |member: &Member| !member.sources.is_empty()
                    && member.sources.iter().all(|source| matches!(source.origin, super::MemberOrigin::Indexer(_)));
                if existing_named && only_indexers(&member) { return }
                if incoming_named && only_indexers(existing) {
                    *existing = member;
                    return;
                }
                existing.required = match (existing.required, member.required) {
                    (Known::Yes(true), _) | (_, Known::Yes(true)) => Known::Yes(true),
                    (Known::Yes(false), Known::Yes(false)) => Known::Yes(false),
                    _ => Known::Unknown,
                };
                for source in member.sources {
                    if !existing.sources.contains(&source) { existing.sources.push(source) }
                }
            },
            None => members.push(member),
        }
    }

    /// The values `at` inherits members from, in declaration order: declared
    /// bases for its view (`members ... inherits`), a product's base, or the
    /// apparent wrapper of a primitive.
    pub fn bases(&self, at: Leaf) -> Known<Vec<NodeValue>> {
        let at = self.modified_source(at).unwrap_or(at);
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        if matches!(at.target, Target::Product(_)) {
            let Known::Yes(Some(product)) = self.product_info(at) else { return Known::Unknown };
            return match self.product_inherited(at, &product) {
                Known::Yes(base) => Known::Yes(base.into_iter().collect()),
                Known::Unknown => Known::Unknown,
            };
        }
        match self.apparent(at) {
            Known::Yes(Some(apparent)) => return Known::Yes(vec![apparent]),
            Known::Yes(None) => {}
            Known::Unknown => return Known::Unknown,
        }
        if matches!(at.target, Target::Traits(_)) { return Known::Yes(Vec::new()) }
        let Known::Yes((node, view)) = self.member_context(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let classes = view.map_or(kind.member_defaults, |(_, classes)| classes);
        let mut bases = Vec::new();
        for (views, base, when) in &kind.member_bases {
            if *views != 0 && classes & views == 0 { continue }
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let base = self.evaluate(at, base);
            if base == NodeValue::Unknown { return Known::Unknown }
            bases.push(base);
        }
        Known::Yes(bases)
    }

    /// The value a leaf without members of its own reads them through
    /// (`apparent`): a primitive's object wrapper, for trait-only leaves and
    /// nodes declaring no members (literals) that fit the trait.
    fn apparent(&self, at: Leaf) -> Known<Option<NodeValue>> {
        if self.tables.apparent.is_empty() { return Known::Yes(None) }
        let recipe = match at.target {
            Target::Traits(bits) => self.tables.apparent.iter().find(|(apparent, _)| bits & apparent != 0).map(|(_, recipe)| recipe),
            Target::Node(node) => {
                let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
                let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
                let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Yes(None) };
                if kind.member_defaults != 0 || !kind.indexed.is_empty() || !kind.member_bases.is_empty() || !kind.member_links.is_empty() {
                    return Known::Yes(None);
                }
                let mut found = None;
                for (bits, recipe) in &self.tables.apparent {
                    match self.fits(at, bits.trailing_zeros() as u16) {
                        Known::Yes(true) => { found = Some(recipe); break }
                        Known::Yes(false) => {}
                        Known::Unknown => return Known::Unknown,
                    }
                }
                found
            }
            Target::Scope(_) | Target::Product(_) | Target::Modified(_) => None,
        };
        let Some(recipe) = recipe else { return Known::Yes(None) };
        // Apparent members are a layer under the value's own: a program
        // without that global (no library) simply has no such layer.
        if let super::Value::Instance { name, classes, .. } = recipe {
            if self.global_absent(at, name, *classes) { return Known::Yes(None) }
        }
        let inherited = self.evaluate(at, recipe);
        if inherited == NodeValue::Unknown { Known::Unknown } else { Known::Yes(Some(inherited)) }
    }

    fn leaf_members(&self, at: Leaf) -> Known<Vec<Member>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        self.memoized(&self.semantic.values.members, at, || self.direct_members(at, MemberRequest::Listing))
    }

    pub(super) fn member_context(&self, at: Leaf) -> Known<(NodeRef, Option<(u32, u64)>)> {
        match at.target {
            Target::Traits(_) | Target::Product(_) | Target::Modified(_) => Known::Unknown,
            Target::Node(node) => Known::Yes((node, None)),
            Target::Scope(scope) => {
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                    return Known::Unknown;
                };
                Known::Yes((NodeRef { file: scope.file, node: info.node }, Some((scope.scope, info.classes))))
            }
        }
    }

    pub(super) fn direct_members(&self, at: Leaf, request: MemberRequest<'_>) -> Known<Vec<Member>> {
        let wanted = request.key();
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        if matches!(at.target, Target::Product(_)) { return self.product_members(at, wanted) }
        if let Target::Modified(id) = at.target { return self.modified_members(id, request) }
        let inherited = match self.apparent(at) {
            Known::Yes(Some(inherited)) => Some(inherited),
            Known::Yes(None) => None,
            Known::Unknown => return Known::Unknown,
        };
        let apparent_members = |inherited: &NodeValue| match wanted {
            Some(key) => match self.keyed_members(inherited, key) {
                Known::Yes(rows) => Known::Yes(rows.into_iter().flatten().collect::<Vec<_>>()),
                Known::Unknown => Known::Unknown,
            },
            None => match self.members(inherited) {
                Known::Yes(rows) => Known::Yes(rows.into_iter().flatten().collect()),
                Known::Unknown => Known::Unknown,
            },
        };
        if matches!(at.target, Target::Traits(_)) {
            return inherited.as_ref().map_or(Known::Yes(Vec::new()), apparent_members);
        }
        // Apparent members (a callable's `Function`) are the lowest layer:
        // own members win, and a node with no member view of its own reads
        // only the apparent type.
        match (self.own_members(at, request), inherited) {
            (own, None) => own,
            (Known::Unknown, Some(inherited)) => apparent_members(&inherited),
            (Known::Yes(mut members), Some(inherited)) => {
                if wanted.is_some() && !members.is_empty() { return Known::Yes(members) }
                let Known::Yes(apparent) = apparent_members(&inherited) else { return Known::Unknown };
                let own: Vec<_> = members.iter().map(|member| member.key.clone()).collect();
                for member in apparent {
                    if !own.contains(&member.key) { Self::merge_member(&mut members, member) }
                }
                Known::Yes(members)
            }
        }
    }

    /// A node's or scope's own members: declared, linked, indexed and from
    /// its member bases, without its apparent type.
    fn own_members(&self, at: Leaf, request: MemberRequest<'_>) -> Known<Vec<Member>> {
        let wanted = request.key();
        match self.publication_members(at, wanted) {
            Known::Yes(Some(members)) => return Known::Yes(members),
            Known::Unknown => return Known::Unknown,
            Known::Yes(None) => {}
        }
        let Known::Yes((node, view)) = self.member_context(at) else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut members = Vec::new();
        if kind.member_defaults == 0 && !kind.indexed.is_empty() {
            let Known::Yes(mut named) = self.indexed_members(at, request, true) else { return Known::Unknown };
            if wanted.is_some() && !named.is_empty() { return Known::Yes(named) }
            let Known::Yes(indexed) = self.indexed_members(at, request, false) else { return Known::Unknown };
            for member in indexed { Self::merge_member(&mut named, member); }
            return Known::Yes(named);
        }
        let Some(names) = self.world.names(node.file) else { return Known::Unknown };
        let Some(index) = self.world.index(node.file) else { return Known::Unknown };
        let (scopes, classes) = match view {
            Some((scope, classes)) => (vec![scope], classes),
            None if kind.member_defaults != 0 => (
                index.opened(node.node).filter(|&scope| names.scopes[scope as usize].classes & kind.member_defaults != 0).collect(),
                kind.member_defaults,
            ),
            // A node with no member view, links, bases or indexers whose
            // open traits do not hold (`null`, `undefined`) has no members
            // of its own; an open one (`any`) may have any.
            None if kind.member_bases.is_empty() && kind.member_links.is_empty() => {
                for &(_, when) in &kind.open_fits {
                    if !self.question.budget.step() { return Known::Unknown }
                    if when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) != Known::Yes(false) {
                        return Known::Unknown;
                    }
                }
                return Known::Yes(Vec::new());
            }
            None => return Known::Unknown,
        };
        for scope in scopes {
            let mut entries: Vec<_> = match wanted {
                Some(Key::Text(key)) => vec![(&**key, index.declared(scope, key))],
                Some(_) => Vec::new(),
                None => index.names(scope).collect(),
            };
            entries.sort_unstable_by_key(|&(key, _)| key);
            for (key, declared) in entries {
                if declared.is_empty() { continue }
                if !self.question.budget.step() { return Known::Unknown }
                let mut sites = Vec::new();
                for &name in declared {
                    if !self.question.budget.step() { return Known::Unknown }
                    let descriptor = names.bindings.get(&name).copied()
                        .or_else(|| names.declared.get(&name).map(|&(_, owner, ..)| owner)).unwrap_or(name);
                    let site = Leaf { target: Target::Node(NodeRef { file: node.file, node: descriptor }), frame: at.frame };
                    if !sites.contains(&site) { sites.push(site) }
                }
                let mut required = Known::Yes(false);
                for &site in &sites {
                    match self.optional(site) {
                        Known::Yes(false) => { required = Known::Yes(true); break }
                        Known::Unknown => required = Known::Unknown,
                        Known::Yes(true) => {}
                    }
                }
                Self::merge_member(&mut members, Member {
                    key: Key::Text(key.into()), sources: sites.into_iter().map(MemberSource::normal).collect(), required,
                });
            }
        }
        for (views, key, value, when) in &kind.member_links {
            if classes & views == 0 { continue }
            if wanted.is_some_and(|wanted| match (wanted, key) {
                (Key::Text(wanted), super::Value::Text(key)) => wanted != key,
                (Key::Ordinal(wanted), super::Value::Ordinal(key)) => wanted != key,
                (_, super::Value::Text(_) | super::Value::Ordinal(_)) => true,
                _ => false,
            }) { continue }
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let Known::Yes(key) = self.key(at, key) else { return Known::Unknown };
            if wanted.is_some_and(|wanted| wanted != &key) { continue }
            let Known::Yes(sites) = self.sites(at, value) else { return Known::Unknown };
            Self::merge_member(&mut members, Member {
                key, sources: sites.into_iter().map(MemberSource::normal).collect(), required: Known::Unknown,
            });
        }
        if wanted.is_some_and(|wanted| members.iter().any(|member| &member.key == wanted)) {
            return Known::Yes(members);
        }
        let Known::Yes(keyed) = self.indexed_members(at, request, true) else { return Known::Unknown };
        for member in keyed { Self::merge_member(&mut members, member); }
        if wanted.is_some() && !members.is_empty() { return Known::Yes(members) }
        if !matches!(request, MemberRequest::Coverage) {
            let own: Vec<_> = members.iter().map(|member| member.key.clone()).collect();
            for (views, base, when) in &kind.member_bases {
                if classes & views == 0 { continue }
                if !self.question.budget.step() { return Known::Unknown }
                match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                let base = self.evaluate(at, base);
                match wanted {
                    Some(key) => {
                        let Known::Yes(mut rows) = self.keyed_members(&base, key) else { return Known::Unknown };
                        if rows.len() != 1 { return Known::Unknown }
                        if let Some(member) = rows.pop().flatten() { Self::merge_member(&mut members, member) }
                    }
                    None => {
                        let rows = if matches!(request, MemberRequest::Shape) { self.shape_members(&base) } else { self.members(&base) };
                        let Known::Yes(mut rows) = rows else { return Known::Unknown };
                        if rows.len() != 1 { return Known::Unknown }
                        for member in rows.pop().unwrap_or_default() {
                            if !own.contains(&member.key) { Self::merge_member(&mut members, member) }
                        }
                    }
                }
            }
        }
        let Known::Yes(indexed) = self.indexed_members(at, request, false) else { return Known::Unknown };
        for member in indexed { Self::merge_member(&mut members, member); }
        Known::Yes(members)
    }

    fn publication_members(&self, at: Leaf, wanted: Option<&Key>) -> Known<Option<Vec<Member>>> {
        let Target::Scope(scope) = at.target else { return Known::Yes(None) };
        let modules = self.semantic.modules();
        let classes = self.question.get(at.frame).map_or(0, |frame| frame.classes);
        if classes == 0 {
            return match modules.publication_names(scope) {
                Known::Yes(None) => Known::Yes(None),
                Known::Yes(Some(_)) | Known::Unknown => Known::Unknown,
            };
        }
        let keys = match wanted {
            Some(Key::Text(key)) => vec![key.clone()],
            Some(_) | None => match modules.publication_names(scope) {
                Known::Yes(None) => return Known::Yes(None),
                Known::Unknown => return Known::Unknown,
                Known::Yes(Some(_)) if wanted.is_some() => return Known::Unknown,
                Known::Yes(Some(keys)) => keys,
            },
        };
        let mut members = Vec::new();
        for key in keys {
            let mut pending = classes;
            let mut targets = Vec::<(Target, u64)>::new();
            loop {
                if !self.question.budget.step() { return Known::Unknown }
                let class = pending & pending.wrapping_neg();
                match modules.namespace_members(scope, &key, class) {
                    Known::Yes(None) => return Known::Yes(None),
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(Some(found)) => for target in found {
                        if let Some((_, classes)) = targets.iter_mut().find(|(existing, _)| *existing == target) {
                            *classes |= class;
                        } else {
                            targets.push((target, class));
                        }
                    },
                }
                pending &= !class;
                if pending == 0 { break }
            }
            let mut sources = Vec::new();
            for (target, classes) in targets {
                if !self.question.budget.step() { return Known::Unknown }
                let (target, owner) = match target {
                    Target::Traits(_) | Target::Product(_) | Target::Modified(_) => return Known::Unknown,
                    Target::Node(node) => {
                        let Known::Yes(owner) = self.declaration_owner(node) else { return Known::Unknown };
                        (Target::Node(owner), owner)
                    },
                    Target::Scope(scope) => {
                        let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                            return Known::Unknown;
                        };
                        (target, NodeRef { file: scope.file, node: info.node })
                    },
                };
                let frame = self.shared_frame(owner, classes)
                    .unwrap_or_else(|| self.question.bind_definition(at.frame, owner, None, None, classes));
                sources.push(MemberSource::normal(Leaf { target, frame }));
            }
            if !sources.is_empty() {
                members.push(Member { key: Key::Text(key), sources, required: Known::Yes(true) });
            }
        }
        Known::Yes(Some(members))
    }
}
