import type Default from 'default';
import type * as Namespace from 'namespace';
import type { A, B as Alias } from 'named';
import type { C } from 'mixed';
import type Mixed from 'mixed';
import type Empty from 'empty';
type Result = Default | Namespace.T | A | Alias | Mixed | C | Empty;
