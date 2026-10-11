import Default from 'default';
import * as Namespace from 'namespace';
import { A, B as Alias } from 'named';
import Mixed, { C } from 'mixed';
import Empty, {} from 'empty';
type Result = Default | Namespace.T | A | Alias | Mixed | C | Empty;
