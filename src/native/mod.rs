//! Native front ends: hand-written parsers for languages whose rules need
//! real syntax (precedence, scopes, declarations). Their nodes are ordinary
//! layers to rules, enabled with a `native <name>` item.

pub mod ts;
