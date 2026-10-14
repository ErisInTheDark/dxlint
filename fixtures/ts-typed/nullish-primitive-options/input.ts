enum N { A = 0, B = 1 }
enum S { A = "", B = "value" }
declare const numeric: N.A | N.B | undefined;
declare const text: S.A | S.B | undefined;
declare const unknownValue: any;
declare const nullableObject: { value: string } | undefined;
numeric || 0;
text || "";
unknownValue || "fallback";
const enumConditional = numeric ? numeric : 1;
const objectFallback = nullableObject || {};
const explicit = numeric != null ? numeric : 0;
declare let optionalText: { n?: { a?: string } };
optionalText.n?.a ? optionalText.n.a : "fallback";
