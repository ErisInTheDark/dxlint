import type { Bridge } from "bridge";
declare const bridge: Bridge;

export async function check() {
  await bridge.read();
  await Promise.all([bridge.read()]);
  await bridge.pending();
  await Promise.all([bridge.pending()]);
}
