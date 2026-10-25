interface Shape {
  draw(scale: number): void;
  name: string;
  draw(scale: string): void;
}

type Factory = {
  create(value: string): Shape;
  label: string;
  create(value: number): Shape;
};
