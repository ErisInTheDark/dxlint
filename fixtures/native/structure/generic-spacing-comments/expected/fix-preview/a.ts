function named/* before */<T>() {}
const called = fn</* open */ T /* close */>/* after */();
const multiline = fn<
  T
>();
const leading = fn<T>();
