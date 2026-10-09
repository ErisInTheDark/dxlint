namespace Case1587 {

class Test {
  private readonly testObj = {
    prop: '',
  };

  public test(): void {
    this.testObj.prop = '';
  }
}
      
}
namespace Case1624 {

class Test {
  readonly #testObj = {
    prop: '',
  };

  public test(): void {
    this.#testObj.prop = '';
  }
}
      
}
namespace Case1660 {

class TestObject {
  public prop: number;
}

class Test {
  private readonly testObj = new TestObject();

  public test(): void {
    this.testObj.prop = 10;
  }
}
      
}
namespace Case1930 {

class Test {
  private readonly testObj = {};
  public test(): void {
    this.testObj.prop.prop = '';
  }
}
      
}
