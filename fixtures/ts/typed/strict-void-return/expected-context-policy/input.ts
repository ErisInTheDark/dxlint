declare const bad: () => number;
declare function optional(cb: (() => void) | (() => undefined)): void;
optional(bad);
declare function dynamic(cb: (() => void) | (() => any)): void;
dynamic(bad);
declare function unknown(cb: (() => void) | (() => unknown)): void;
unknown(bad);
const mixed: (() => void) | false = bad;

class Base {
  cb() {}
  static cb() {}
}
class Derived extends Base {
  cb() { return 1; }
  static cb() { return 2; }
}
