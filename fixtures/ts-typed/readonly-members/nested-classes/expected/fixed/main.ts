namespace Case900 {

class TestIncorrectlyModifiableInline {
  private readonly incorrectlyModifiableInline = 7;

  public createConfusingChildClass() {
    return class {
      private readonly incorrectlyModifiableInline = 7;
    };
  }
}
      
}
namespace Case946 {

class TestIncorrectlyModifiableInline {
  readonly #incorrectlyModifiableInline = 7;

  public createConfusingChildClass() {
    return class {
      readonly #incorrectlyModifiableInline = 7;
    };
  }
}
      
}
