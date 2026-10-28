use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use crate::expr::AttrMemo;
use super::frontend::Native;
use super::units::Unit;
use super::{Linter, Run};

pub(super) struct Arena<'a> {
    linter: &'a Linter,
    #[expect(clippy::vec_box, reason = "views must keep their address while the vec grows")]
    items: RefCell<Vec<Box<UnitView<'static>>>>,
}

pub(super) struct UnitView<'a> {
    native: OnceCell<Option<Box<dyn Native + 'a>>>,
    pub memo: AttrMemo<'a>,
    pub unit: Arc<Unit>,
    pub reindented: RefCell<HashMap<usize, String>>,
}

impl<'a> Arena<'a> {
    pub fn new(linter: &'a Linter) -> Self {
        Self { linter, items: RefCell::new(Vec::new()) }
    }

    pub fn insert(&self, unit: Arc<Unit>) -> u32 {
        let mut items = self.items.borrow_mut();
        let id = u32::try_from(items.len()).expect("arena file limit");
        items.push(Box::new(UnitView { native: OnceCell::new(), memo: AttrMemo::default(), unit, reindented: RefCell::default() }));
        id
    }

    /// Forgets every view (none may be borrowed).
    pub fn clear(&self) {
        self.items.borrow_mut().clear();
    }

    /// Every file's unit, by id.
    pub fn units(&self) -> Vec<Arc<Unit>> {
        self.items.borrow().iter().map(|item| item.unit.clone()).collect()
    }

    /// The view of file `id`, without its native front-end view: trees,
    /// texts and paths need none.
    pub fn get<'v>(&'v self, id: u32) -> Option<&'v UnitView<'v>> {
        let item = self.items.borrow().get(id as usize).map(|item| &**item as *const UnitView<'static>)?;
        // SAFETY: boxes never move, are removed or replaced. Stored lifetimes
        // are erased only inside this owner; borrowers see the arena lifetime.
        // Only `with_native` builds native views, borrowing the owned unit and
        // the linter held by the arena. Native and memo drop before their unit;
        // the linter outlives the arena. No erased reference can escape.
        Some(unsafe { &*item.cast::<UnitView<'v>>() })
    }

    /// The view of file `id` with its native front-end view built, for
    /// running expressions on it.
    pub fn with_native<'v>(&'v self, id: u32) -> Option<&'v UnitView<'v>> {
        let view = self.get(id)?;
        view.native.get_or_init(|| {
            let build = || {
                let unit = &view.unit;
                let front = unit.parsed.front?;
                let run = Run { path: &unit.path, lang: unit.lang, text: &unit.text, lines: unit.lines(), file: &unit.rel, active: &[], settings: None };
                Some(self.linter.native_facts(front, &unit.parsed.tree, &run, (&*unit.path, std::path::Path::new(&unit.rel)), unit.parsed.state.clone()))
            };
            match &self.linter.profile {
                Some(profile) => profile.time_semantic_query("units.native", build),
                None => build(),
            }
        });
        Some(view)
    }
}

impl<'a> UnitView<'a> {
    pub fn native(&'a self) -> Option<&'a (dyn Native + 'a)> {
        self.native.get().and_then(Option::as_deref)
    }
}
