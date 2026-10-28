export function annotated(): number { return 1; }
export function inferred() { return 1; }
export function other(): number { const half = 1 / 2; return half; }
declare global { function annotated(): string; }
