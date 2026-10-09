namespace Case424 {

class TestReadonlyParameter {
  public constructor(private readonly correctlyReadonlyParameter = 7) {}
}
    
}
namespace Case429 {

class TestCorrectlyModifiableParameter {
  public constructor(private correctlyModifiableParameter = 7) {}

  public mutate() {
    this.correctlyModifiableParameter += 1;
  }
}
    
}
namespace Case1447 {

class TestIncorrectlyModifiableParameter {
  public constructor(private readonly incorrectlyModifiableParameter = 7) {}
}
      
}
namespace Case1471 {

class TestIncorrectlyModifiableParameter {
  public constructor(
    public ignore: boolean,
    private readonly incorrectlyModifiableParameter = 7,
  ) {}
}
      
}
