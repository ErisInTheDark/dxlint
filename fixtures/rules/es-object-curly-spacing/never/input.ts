import { a, b } from "module";
export { a, b };
const plain = { a:1 };
const tight = {a:1};
const comment = { /* inside space */a:1/* trailing space */ };
const adjacent = {/*x*/a:1/*y*/};
const multiline = {
  a:1
};
const array = {a:[]};
const nested = {a:{b:1}};
const empty = { };
type Shape = {a:number};
interface Shape2 { a:number }
type Mapped = {[K in keyof Shape]:Shape[K]};
