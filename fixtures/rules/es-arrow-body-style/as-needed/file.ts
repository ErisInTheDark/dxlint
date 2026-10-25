const café = "λ"; const a = () => { return value; }; const b = () => { return other; };
const object = () => { return { x: 1 }.x; };
const sequence = () => { return a, b; };
for (let f = () => { return key in object; };;);
for (;f = () => { return key in object; };);
const comments = () => { /* before */ return /* value */ x /* after */; /* end */ };
const line = () => {
  // before
  return value; // after
};
const nested = () => { return () => { return value; }; };
