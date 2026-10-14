declare const pending: Promise<void>; interface Promise<T> { then(): T; }
export default function read(): Promise<void> { return pending; }
const arrow = (p: Promise<void>) => p;
const expression = function (p: Promise<void>) { return p; };
const object = { read(): Promise<void> { return pending; } };
class Reader {
  public'bar'(): Promise<void> { return pending; }
  private['baz'](): Promise<void> { return pending; }
  static protected[(1)](): Promise<void> { return pending; }
  @decorator
  public read(): Promise<void> { return pending; }
}
function unknownReturn(): unknown { return pending; }
function anyReturn(): any { return pending; }
async function existing(): Promise<void> { return pending; }
