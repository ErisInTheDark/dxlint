interface Function { apply: any; (): void; } declare const broad: Function; declare const narrow: () => void; broad as () => void; narrow as Function;
