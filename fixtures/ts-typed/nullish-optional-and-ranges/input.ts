declare let x: { n?: { a?: string } };
declare const y: string;
x.n?.a ? x?.n?.a : y;
x.n?.a ? x.n.a : y;
declare let maybe: object | null;
maybe || {};
maybe ||= {};
maybe ? maybe : {};
declare const nonnull: string;
nonnull ? nonnull : y;
x.n?.a ? y : x?.n?.a;
if (x.n?.a || y) {}
Boolean(x.n?.a || y);
maybe
  ? maybe
  : {};
