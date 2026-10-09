namespace Case53 {

class TestModifiableStatic {
  private static correctlyModifiableStatic = 7;

  public constructor() {
    TestModifiableStatic.correctlyModifiableStatic += 1;
  }
}
    
}
namespace Case62 {

class TestModifiableStatic {
  static #correctlyModifiableStatic = 7;

  public constructor() {
    TestModifiableStatic.#correctlyModifiableStatic += 1;
  }
}
    
}
namespace Case693 {

class TestBothIntersection {
  private prop1: number = 1;
  private static prop2: number;

  test() {
    const that = {} as typeof TestBothIntersection & this;
    that.prop1 = 1;
    that.prop2 = 1;
  }
}
    
}
namespace Case705 {

class TestBothIntersection {
  private prop1: number = 1;
  private static prop2: number;

  test() {
    const that = {} as this & typeof TestBothIntersection;
    that.prop1 = 1;
    that.prop2 = 1;
  }
}
    
}
namespace Case852 {

class TestIncorrectlyModifiableStaticArrow {
  private static readonly incorrectlyModifiableStaticArrow = () => 7;
}
      
}
