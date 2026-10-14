import * as ns from "pkg";
import normal, { value as named, \u0066oo as escaped, "str\u0069ng" as stringName } from "pkg";
ns.x = 1;
(ns).x = 1;
((ns))["x"]++;
delete (ns).x;
for ((ns).x of values) {}
({ value: (ns).x } = source);
Object.assign(ns, source);
Object.assign((ns), source);
Reflect.set((ns), "x", 1);
Object.freeze((ns));
ns.nested.x = 1;
Object.assign(target, ns);
normal.x = 1;
named.x = 1;
function local(ns) { ns.x = 1; Object.assign(ns, source); }
function shadow(Object) { Object.assign(ns, source); }
ns = normal;
probe(normal, named, escaped, stringName, ns);
probe((normal), (named), (escaped), (stringName), (ns));
import legacy = require("legacy");
probe(legacy);
function probeShadow(ns) { probe(ns); }
