namespace Case1144 {

class TestIncorrectlyModifiablePostMinus {
  private readonly incorrectlyModifiablePostMinus = 7;

  public mutate() {
    this.incorrectlyModifiablePostMinus - 1;
  }
}
      
}
namespace Case1209 {

class TestIncorrectlyModifiablePostPlus {
  private readonly incorrectlyModifiablePostPlus = 7;

  public mutate() {
    this.incorrectlyModifiablePostPlus + 1;
  }
}
      
}
namespace Case1275 {

class TestIncorrectlyModifiablePreMinus {
  private readonly incorrectlyModifiablePreMinus = 7;

  public mutate() {
    -this.incorrectlyModifiablePreMinus;
  }
}
      
}
