class C<T extends number> {
  get x(): T { return 1 as T; }
  set x(value: string) {}
}
