type Mapper = (value: number) => number;
export const mapper: Mapper = value => value + 1;

const literal: (text: string) => number = text => text.length;

const annotated: (value: number) => void = (value: string) => {};
