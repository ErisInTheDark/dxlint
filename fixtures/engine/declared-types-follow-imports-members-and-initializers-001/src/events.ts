export namespace Emitter {
  export class Host<E> {
    on(event: E): this { return this }
  }
}
