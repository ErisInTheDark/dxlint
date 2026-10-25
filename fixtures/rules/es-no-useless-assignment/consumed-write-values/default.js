const obj = { a: 1 };
let {
  a,
  b = (a = 2)
} = obj;
a = 3;
console.log(a, b);
