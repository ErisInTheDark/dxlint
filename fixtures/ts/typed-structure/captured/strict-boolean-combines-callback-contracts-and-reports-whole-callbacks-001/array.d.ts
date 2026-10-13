interface Array<T> {
  filter(predicate: (value: T) => unknown): T[];
  some(predicate: (value: T) => unknown): boolean;
  every(predicate: (value: T) => unknown): boolean;
}
