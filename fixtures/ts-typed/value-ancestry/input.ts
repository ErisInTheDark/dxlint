interface Derived extends Promise<number> {}
interface Tagged extends Derived {}
type Both = Promise<number> & { tag: string };
interface Combined extends Both {}
declare const real: Tagged;
declare const intersection: Combined;
interface ProjectOnly {}
declare const project: ProjectOnly;
namespace Shadow {
  interface Promise<T> { local: T }
  declare const shadow: Promise<number>;
}
declare const scalar: number;
interface Broken extends Missing {}
declare const broken: Broken;
declare const factory: typeof LibraryFactory;
declare const factoryInstance: LibraryFactory;
class ChildFactory extends LibraryFactory {}
const derivedFactory = ChildFactory;
