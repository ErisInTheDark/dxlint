import type { f } from "./source";
import type * as NS from "./source";
type F = typeof f;
type V = typeof NS.runtimeValue;
f();
NS.runtimeValue;
