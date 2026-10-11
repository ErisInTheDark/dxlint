import type /* declaration */ Default from 'default';
import type * as Namespace from 'namespace';
import type { A } from 'named';
import { type /* inline */ B, type C as Alias } from 'inline';
type Result = Default | Namespace.T | A | B | Alias;
