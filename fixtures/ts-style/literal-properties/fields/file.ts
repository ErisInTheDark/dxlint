declare const computed: string;
declare const dynamic: string;
declare function make(): string;
class Sample {
  get greeting() {
    return 'hello';
  }
  get [computed]() {
    return `template`;
  }
  static get labelled() {
    return 'static';
  }
  readonly plain = 'value';
  readonly [dynamic] = `braced`;
  private readonly prefixed = 'private';
  get paired() {
    return 'paired';
  }
  set paired(value: string) {}
  readonly rewritten: string;
  constructor() {
    this.rewritten = 'later';
  }
  get call() {
    return make();
  }
}
