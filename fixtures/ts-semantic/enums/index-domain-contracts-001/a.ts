enum Fruit { Apple }
enum Vegetable { Asparagus = "asparagus" }
declare const numbers: number[];
declare const numberIndex: { [key: number]: number };
declare const textIndex: { [key: string]: string };
declare const fruits: Fruit[];
declare const vegetables: { [key: string]: Vegetable };
const arrayTransfer: { [key: number]: Fruit } = numbers;
const ordinalTransfer: { [key: number]: Fruit } = numberIndex;
const textTransfer: { [key: string]: Vegetable } = textIndex;
const safeArray: { [key: number]: Fruit } = fruits;
const safeText: { [key: string]: Vegetable } = vegetables;
const numberUnion: { [key: number]: Fruit | number } = numbers;
const textUnion: { [key: string]: Vegetable | string } = textIndex;
export {};
