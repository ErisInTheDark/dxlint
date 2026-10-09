type Record<K extends keyof any, T> = { [P in K]: T }; declare const indexed: Record<string, string>; const { value = 'fallback' } = indexed;
