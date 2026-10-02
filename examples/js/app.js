// TODO: real logging
const same = (a) => a == "==" ? `${a != null ? "x == y" : (b) => b}` : a;
console.log(same("(x) => x"));
if (same(1) === 1 && same(2) !== 2) {
  console.debug("strict already");
}
