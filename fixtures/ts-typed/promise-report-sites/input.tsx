declare function consume(callback: () => void): void;
consume(async () => {});
let variable: () => void = async () => {};
declare const object: { optional?: () => void };
object.optional = async () => {};
const properties: { inferred: () => void; typed: () => void; method(): void } = {
  inferred: async () => {},
  typed: async (): Promise<void> => {},
  async method() {}
};
function returns(): () => void { return async () => {}; }
interface Base { run(): void; }
class Derived implements Base { async run() {} }
declare function View(props: { onDone: () => void }): unknown;
const view = <View onDone={async () => {}} />;
