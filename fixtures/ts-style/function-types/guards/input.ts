export default interface Α { /** keep */ (): void; }
interface B { (x: this): void; }
interface C { (): this | undefined; }
interface D { (): void; property: number; }
interface E { (): void; (): string; }
interface F extends Base { (): void; }
