import { A, B, type Already } from 'named';
import Value, { C as Alias, D } from 'mixed';
import Default, { E, F } from 'all';
const value = B() + Value();
type Result = A | Already | Alias | D | Default | E | F;
