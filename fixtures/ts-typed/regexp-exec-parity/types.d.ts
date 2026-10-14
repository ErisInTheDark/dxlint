interface Array<T> { [key: number]: T; length: number; }
interface RegExp { exec(input: string): string[] | null; }
declare const RegExp: { new(pattern: string, flags?: string): RegExp };
