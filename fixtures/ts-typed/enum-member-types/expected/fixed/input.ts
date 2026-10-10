import { E } from "./base";
declare const zero: E.Zero;
declare const text: E.Text;
if (zero) {}
if (text) {}
import * as ns from "./base";
import Alias = ns.E;
declare const aliasZero: Alias.Zero;
if (aliasZero) {}
const primitiveToEnum = 0 as E.Zero;
const sameEnum = zero;
