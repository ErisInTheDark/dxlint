type Α = () => string; type B = (x: number) => void;
type C<T extends { tag: string } = { tag: string }> = (x: T) => string;
type D = () => void;
type E<T> = (this: T) => void;
export type F = <T>(x: T) => T;
type G = () => { nested: this };
