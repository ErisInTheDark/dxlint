import { Item as Allowed } from "./allowed";
import { Item as Other } from "./other";
type NestedAllowed = { readonly item: Allowed };
type NestedOther = { readonly item: Other };
type WritableParent = { item: Allowed };
function allow(value: Allowed) {}
function other(value: Other) {}
function child(value: NestedAllowed) {}
function inner(value: NestedOther) {}
function mixed(value: Allowed | Other) {}
function write(value: WritableParent) {}
