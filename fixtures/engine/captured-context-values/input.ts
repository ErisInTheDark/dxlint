declare const nullable: string[] | null;
declare function schedule(callback: (section: { value: string | null }) => void): void;
schedule(section => {
  section.value = null;
});
if (nullable) {
  const direct = nullable[0];
  const read = () => {
    const captured = nullable[0];
  };
}

declare const items: { name: string }[];
for (const item of items) {
  const name = item.name;
}

declare const tuple: readonly [{ name: string }, { name: number }];
for (const item of tuple) { const tupleName = item.name; }
declare const custom: { [Symbol.iterator](): Iterator<{ name: boolean }> };
for (const item of custom) { const customName = item.name; }
declare const text: string;
for (const item of text) { const character = item; }
