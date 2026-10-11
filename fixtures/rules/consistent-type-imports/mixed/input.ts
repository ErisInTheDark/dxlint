import { A, B, C } from 'named';
import Default, { Value, Type } from 'mixed';
import Unused, * as Namespace from 'namespace';
const value = B() + Value();
type Result = A | C | Default | Type | Namespace.T;
