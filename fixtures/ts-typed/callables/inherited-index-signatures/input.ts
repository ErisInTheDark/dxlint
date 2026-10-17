interface Indexed {
  [key: string]: () => void;
}
interface NumberIndexed {
  [key: number]: () => void;
}
interface Named {
  run(): void;
  callback: () => void;
}
interface Inherited extends Named {}
interface OnlyIndexed extends Indexed {
  run(): Promise<void>;
}
interface OnlyNumberIndexed extends NumberIndexed {
  0(): Promise<void>;
}
interface Mixed extends Indexed, Inherited {
  run(): Promise<void>;
  callback: () => Promise<void>;
  extra(): Promise<void>;
}
declare class IndexedBase {
  [key: string]: () => void;
}
declare class IndexedDerived extends IndexedBase {
  extra(): Promise<void>;
}
declare class Implemented implements Indexed, Named {
  run(): Promise<void>;
  callback: () => Promise<void>;
  extra(): Promise<void>;
}
