import /* declaration */ Default from 'default';
import * as Namespace from 'namespace';
import { A } from 'named';
import { /* inline */ B, C as Alias } from 'inline';
type Result = Default | Namespace.T | A | B | Alias;
