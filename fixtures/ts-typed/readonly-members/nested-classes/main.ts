namespace Case900 {

class TestIncorrectlyModifiableInline {
  private incorrectlyModifiableInline = 7;

  public createConfusingChildClass() {
    return class {
      private incorrectlyModifiableInline = 7;
    };
  }
}
      
}
namespace Case946 {

class TestIncorrectlyModifiableInline {
  #incorrectlyModifiableInline = 7;

  public createConfusingChildClass() {
    return class {
      #incorrectlyModifiableInline = 7;
    };
  }
}
      
}
