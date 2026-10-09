namespace Case992 {

class TestIncorrectlyModifiableDelayed {
  private incorrectlyModifiableDelayed = 7;

  public constructor() {
    this.incorrectlyModifiableDelayed = 7;
  }
}
      
}
namespace Case1024 {

class TestIncorrectlyModifiableDelayed {
  #incorrectlyModifiableDelayed = 7;

  public constructor() {
    this.#incorrectlyModifiableDelayed = 7;
  }
}
      
}
namespace Case155 {

class TestModifiableDelayed {
  private correctlyModifiableDelayed = 7;

  public mutate() {
    this.correctlyModifiableDelayed += 1;
  }
}
    
}
