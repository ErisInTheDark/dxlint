function empty() {}
void empty();
const box = { value: "text" };
void box;
void (box.value as string);
void (() => {})();
void 0;
declare const pending: { then(): void };
void pending;
empty()
void (() => {})();
void { value: 1 };
void /* retain */ empty();
