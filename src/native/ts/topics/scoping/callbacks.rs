//! Array factory syntax shared by receiver analysis.

use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn array_factory(file: &NativeFile, callee: u32) -> bool {
    let s = &file.syntax;
    let callee = s.unparen(callee);
    let method = crate::native::ts::statics::property_name(s, callee);
    s.get(callee, F::Object).is_some_and(|n| {
        let n = s.unparen(n);
        s.is(n, K::Ident) && match method.as_deref() {
            Some("from") => s.text(n).ends_with("Array"),
            Some("fromAsync") => s.text(n) == "Array",
            _ => false,
        }
    })
}
