function shorthand() {
  let a = 0, b = 0, c = 0;
  consume(a, b, c);
  ({ a = 1, foo: b, ...c } = source());
}
function selfDefault() {
  let a = 0;
  ({ a = a + 1 } = source());
}
function declaration() {
  let { a = 1 } = source();
  consume(a);
  a = 2;
}
function aliasDefault() {
  let a = 0;
  consume(a);
  ({ foo: a = 1 } = source());
}
