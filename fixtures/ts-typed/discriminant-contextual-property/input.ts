declare const a: "1" | "2";
const s: { type: "a"; a: string } | { type: "b" } = { type: "a", a: a as string };
