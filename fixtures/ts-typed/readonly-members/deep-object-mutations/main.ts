namespace Case1587 {

class Test {
  private testObj = {
    prop: '',
  };

  public test(): void {
    this.testObj.prop = '';
  }
}
      
}
namespace Case1624 {

class Test {
  #testObj = {
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
  private testObj = new TestObject();

  public test(): void {
    this.testObj.prop = 10;
  }
}
      
}
namespace Case1930 {

class Test {
  private testObj = {};
  public test(): void {
    this.testObj.prop.prop = '';
  }
}
      
}
