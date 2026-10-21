declare const computed: string;
class Sample {
  readonly plain = 'value';
  readonly [computed] = `braced`;
  private readonly prefixed = 'private';
  static readonly fixed = 'static';
  get getter() {
    return 'literal';
  }
}
