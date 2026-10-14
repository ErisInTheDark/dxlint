declare const pending: Promise<void>; interface Promise<T> { then(): T; }
export default async function read(): Promise<void> { return pending; }
const arrow = async (p: Promise<void>) => p;
const expression = async function (p: Promise<void>) { return p; };
const object = { async read(): Promise<void> { return pending; } };
class Reader {
  public async 'bar'(): Promise<void> { return pending; }
  private async ['baz'](): Promise<void> { return pending; }
  static protected async [(1)](): Promise<void> { return pending; }
  @decorator
  public async read(): Promise<void> { return pending; }
}
function unknownReturn(): unknown { return pending; }
function anyReturn(): any { return pending; }
async function existing(): Promise<void> { return pending; }
