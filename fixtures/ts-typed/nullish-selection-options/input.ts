declare const n: number | undefined;
if (n || 1) {}
Boolean(n || 2);
(n && 1) || 3;
const ternary = n ? n : 4;
let value: number | undefined;
if (!value) { value = 5; }
const normal = n || 6;
