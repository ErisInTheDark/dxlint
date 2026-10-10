interface Α { (): string; } interface B { (x: number): void; }
interface C<T extends { tag: string } = { tag: string }> { (x: T): string; }
interface D extends Function { (): void; }
interface E<T> { (this: T): void; }
export interface F { <T>(x: T): T; }
interface G { (): { nested: this }; }
