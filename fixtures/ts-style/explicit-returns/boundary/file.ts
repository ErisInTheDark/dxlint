export function exported(arg) {
  return arg;
}
export const assigned = (arg) => arg;
export class Sample {
  constructor(public parameter) {}
  method(a) {
    return a;
  }
  accessor arrow = (a) => a;
  accessor fn = function (a) {
    return a;
  };
}
export abstract class Abstract {
  abstract empty(a);
  abstract hinted(a): void;
  abstract rested(...rest: any[]): void;
}
