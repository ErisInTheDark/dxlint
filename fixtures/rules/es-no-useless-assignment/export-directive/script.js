/* exported foo */
let foo = 0;
console.log(foo);
foo = 1;
function inner() {
  let foo = 0;
  console.log(foo);
  foo = 1;
}
