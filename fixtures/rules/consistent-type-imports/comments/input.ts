import Default /* keep default */, /* keep named */ { A, Value } from 'mixed';
import { /* keep first */ B, Other, /* keep last */ C } from 'named';
const value = Value() + Other();
type Result = Default | A | B | C;
