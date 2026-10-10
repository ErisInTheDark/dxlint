function* f() {
  yield * value;
  yield/* before */ * /* after */value;
  yield * value;
  yield value;
}
