import type { A} from 'mixed';
import type Default /* keep default */ from 'mixed';
import /* keep named */ { Value } from 'mixed';
import type { /* keep first */ B, /* keep last */ C } from 'named';
import { Other } from 'named';
const value = Value() + Other();
type Result = Default | A | B | C;
