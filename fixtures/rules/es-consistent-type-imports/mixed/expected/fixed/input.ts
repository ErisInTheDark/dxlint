import type { A, C } from 'named';
import { B } from 'named';
import type { Type } from 'mixed';
import type Default from 'mixed';
import { Value } from 'mixed';
import type * as Namespace from 'namespace';
import Unused from 'namespace';
const value = B() + Value();
type Result = A | C | Default | Type | Namespace.T;
