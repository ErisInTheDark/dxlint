function* f() {
  yield* value;
  yield* /* keep */ value;
  yield/* stay */*value;
  yield*
    value;
  yield value;
}
