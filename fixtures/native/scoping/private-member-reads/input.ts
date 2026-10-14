class Mixed {
  #object;
  #array;
  #rest;
  private object;
  private array;
  private rest;
  private fromThis = 0;
  private fromAlias = 0;
  private fromParameter = 0;
  private static fromClass = 0;
  private "quoted" = 0;
  private accessor auto = 0;
  method(other: Mixed) {
    ({ value: this.#object } = input);
    [this.#array] = input;
    [...this.#rest] = input;
    ({ value: this.object } = input);
    [this.array] = input;
    [...this.rest] = input;
    const { fromThis } = this;
    const self = this;
    consume(self.fromAlias);
    consume(other.fromParameter);
    consume(Mixed.fromClass);
    consume(this["quoted"]);
    this.auto = 1;
  }
}
class Parameters {
  constructor(private used: number, private unused: number) { consume(this.used); }
}
class Contexts {
  private reassigned = 0;
  private wrongSide = 0;
  private nestedFunction = 0;
  private arrow = 0;
  private explicitThis = 0;
  private static queried = 0;
  method(other: typeof Contexts) {
    let self = this;
    self = replacement;
    consume(self.reassigned);
    const read = () => this.arrow;
    function detached() { consume(this.nestedFunction); }
    function attached(this: Contexts) { consume(this.explicitThis); }
    consume(other.queried);
  }
  static read() { consume(this.wrongSide); }
}
