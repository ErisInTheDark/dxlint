interface Function { apply: any; } declare const broad: Function; declare const narrow: () => void; broad as () => void; narrow as Function;
