class C {
  private a = 1;
  private b = 2;
  #c = 3;
  constructor() { this.b = 3; setTimeout(() => { this.a = 4; }); }
  m(other: C) { other.b = 5; return this.#c; }
}
class D extends C { constructor(x: boolean) { if (x) { super(); } } }
class E extends C { constructor() { super(); } }
function p() { return this; }
function q() { return () => arguments; }
function* r() { yield 1; }
