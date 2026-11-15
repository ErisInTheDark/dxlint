import { Loader } from './modules';
declare function verify(value: unknown): void;
export async function check(key: string) {
  const { value: renamed } = await import('./modules');
  verify(renamed);
  const { payload: { outer: { value: nested } } } = await import(`./modules`);
  verify(nested);
  const { payload } = await import(('./\x6dodules'));
  const [, second] = payload.pair;
  verify(second);
  const { maybe: fallback = 0 } = payload;
  verify(fallback);
  const module = await new Loader().pending;
  verify(module.value);
  const { default: transport } = await import('transport');
  verify(transport.value);
  const { ...rest } = payload;
  verify(rest);
  const { [key]: computed } = payload;
  verify(computed);
}
