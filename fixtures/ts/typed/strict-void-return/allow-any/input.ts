declare function consume(cb: () => void): void;
declare const arbitrary: () => any;
consume(arbitrary);
consume(() => (1 as any));
consume(() => null);
consume(() => undefined);
