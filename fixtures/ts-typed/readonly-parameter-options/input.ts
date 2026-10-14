interface Array<T> { length: number; [index: number]: T }
class Properties {
  constructor(private hidden: string[], exposed: string[]) {}
}
type Callback<T> = (value: T) => void;
declare function contextual(callback: Callback<string[]>): void;
contextual(value => {});
contextual((value: string[]) => {});
function methodValue(arg: { method(): void }) {}
function mutableProperty(arg: { value: string }) {}
