use crate::expr::Value;
use crate::names::question::{Known, Leaf};
use crate::names::semantic::Semantic;

use super::QueryFunction;

pub(super) static FUNCTIONS: &[QueryFunction] = &[
    QueryFunction { name: "command_output", arity: 3, call: output },
];

/// `command_output(node, name, args)`: what the root config's command
/// prints for `args` (a list of texts): `{ state, output, message }`, state
/// `ready`, `failed`, `pending` (running; the file is linted again when it
/// finishes) or `disabled`.
fn output(semantic: &Semantic<'_>, _: Leaf, args: &[Value<'static>]) -> Known<Value<'static>> {
    let [name, arguments] = args else { return Known::Unknown };
    let arguments = arguments.item_slice().iter().map(|arg| arg.text().into_owned()).collect::<Vec<_>>();
    semantic.world.command_output(&name.text(), &arguments)
}
