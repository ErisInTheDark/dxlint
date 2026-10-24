type Mapper = (value: number) => number;
export const direct: Mapper = value => value + 1;

interface Options { idOf(item: { id: number }): number }
export const options: Options = { idOf: item => item.id };

declare function decorate(run: (text: string) => string): string;
export const decorated = decorate(text => text);
