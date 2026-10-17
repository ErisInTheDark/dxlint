interface Source {
  /** @deprecated */
  old: string;
  fresh: string;
  /** @deprecated */
  method(): void;
  nested: Source;
}
declare const source: Source;
declare const unknownSource: unknown;
declare const anySource: any;
const key = "old";
source[key];
source[`old`];
source["method"];
const { old } = source;
const { old: renamed } = source;
const { old: fallback = "" } = source;
const { nested: { old: nested } } = source;
const { fresh } = source;
source["fresh"];
const { old: uncertain } = unknownSource;
const { old: unchecked } = anySource;
declare const dynamicKey: string;
source[dynamicKey];
function read({ old: parameter }: Source) {}
const [{ old: arrayMember }] = [source];
let target = "";
({ old: target } = source);
const { "old": quoted } = source;
function readDefault({ old: parameter } = source) {}
/** @deprecated */
class Legacy {
  /** @deprecated */
  field: string;
}
declare const legacy: Legacy;
const { field } = legacy;
source[`${key}`];
