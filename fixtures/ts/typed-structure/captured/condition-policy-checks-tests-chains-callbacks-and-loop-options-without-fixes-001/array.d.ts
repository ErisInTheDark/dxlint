interface Array<T> {
  filter(predicate: (value: T) => unknown): T[];
}
