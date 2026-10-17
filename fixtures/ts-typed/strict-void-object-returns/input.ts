declare function accept(cb: () => void): void;
accept(function () {
  return { a: 1 };
});
accept(function () {
  return undefined;
});
