
function* yieldNumbers() {
  yield 1;
  yield 2;
  yield 3;
}
for await (const value of yieldNumbers()) {
  console.log(value);
}
for await (const value of (yieldNumbers())) /* gap */ {}
