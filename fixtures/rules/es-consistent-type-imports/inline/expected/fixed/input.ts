import { type A, B, type Already } from 'named';
import Value, { type C as Alias, type D } from 'mixed';
import {type E, type F} from 'all';
import type Default from 'all';
const value = B() + Value();
type Result = A | Already | Alias | D | Default | E | F;
