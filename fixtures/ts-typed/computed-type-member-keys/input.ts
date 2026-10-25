enum Bus { A, B }
interface Methods { [Bus.A](): number; [Bus.B]?: string }
type Shape = { [Bus.A](): number; [Bus.B]?: string };
declare const fromInterface: Methods;
declare const fromLiteral: Shape;
export {};
