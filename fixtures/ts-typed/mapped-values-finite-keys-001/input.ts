type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Exclude<T, U> = T extends U ? never : T;
type Omit<T, K extends keyof any> = Pick<T, Exclude<keyof T, K>>;
type Record<K extends keyof any, V> = { [P in K]: V };
type Partial<T> = { [P in keyof T]?: T[P] };
type Required<T> = { [P in keyof T]-?: T[P] };
interface Shape { value: any; safe: number; optional?: number }
declare const picked: Pick<Shape, 'value' | 'safe'>;
declare const omitted: Omit<Shape, 'safe'>;
declare const record: Record<'left' | 'right', any>;
picked.value;
picked.safe;
picked.optional;
omitted.value;
omitted.safe;
record.left;
record.right;
record.missing;
function optional({ value = 0 }: Partial<{ value: number }>) {}
function required({ value = 0 }: Required<Partial<{ value: number }>>) {}
function preserved({ optional = 0 }: Pick<Shape, 'optional'>) {}
function removed({ optional = 0 }: Required<Pick<Shape, 'optional'>>) {}
