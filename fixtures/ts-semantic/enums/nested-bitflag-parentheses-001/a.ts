enum Flags {
  Read = 1 << 0,
  Write = 1 << 1,
}
const combined: Flags = Flags.Read | (Flags.Write & Flags.Read);
const nested: Flags = (Flags.Read | ((Flags.Write & Flags.Read)));
const complemented: Flags = Flags.Read | (~(Flags.Write));
const unsafe: Flags = Flags.Read | (Flags.Write & 4);
void combined;
void nested;
void complemented;
void unsafe;
export {};
