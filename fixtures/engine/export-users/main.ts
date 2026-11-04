import Thing, { used, aliased as other } from "./lib";
import { viaBarrel, starUsed } from "./barrel";
import * as whole from "./namespaced";
import required = require("./required");
import { one } from "./cycle-a";
import { shared } from "./only-old";
import { y } from "./reexported";
import "./side-effect";
export type {};

type T = import("./typed").T;
declare module "./augmented" {
  interface Extra { x: number }
}

const later = import("./dynamic");
console.log(Thing, used, other, viaBarrel, starUsed, whole, required, one, shared, y, later, null as unknown as T);
