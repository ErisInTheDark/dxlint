interface I {
  field: number;
  method(): void;
  (): void;
  [key: string]: number;
}
type T = {
  field: string;
}
