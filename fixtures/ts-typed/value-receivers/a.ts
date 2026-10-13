function explicit(this: { value: string }) {
  const explicitValue = this.value;
  const arrowValue = (() => this.value)();
}
function implicit() { const implicitValue = this; }
class Holder {
  value: string = "";
  static count: number = 0;
  method() { const instance = this; const instanceValue = this.value; }
  static method() { const staticValue = this.count; }
}
declare const contextual: (this: { value: string }) => void;
const callback: typeof contextual = function () { const contextualValue = this.value; };
const object = { value: "", method() { const objectValue = this.value; } };
