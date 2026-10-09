interface Overloaded { (): void; (value: string): undefined; }
((value => {}) as Overloaded)('');