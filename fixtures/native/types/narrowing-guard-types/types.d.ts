interface Array<T> { [index: number]: T; }
interface ArrayConstructor { isArray(arg: any): arg is any[]; }
declare var Array: ArrayConstructor;
