import { C } from "./defs";
declare const x: C;
if (typeof x.options.delay === "boolean") {
  x.options.delay = x.options.delay === true ? 13 : 0;
}
