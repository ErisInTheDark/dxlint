namespace Case1406 {

class TestOverlappingClassVariable {
  private readonly overlappingClassVariable = 7;

  public workWithSimilarClass(other: SimilarClass) {
    other.overlappingClassVariable = 7;
  }
}

class SimilarClass {
  public overlappingClassVariable = 7;
}
      
}
namespace Case652 {

class TestIntersection {
  private prop: number = 3;

  test() {
    const that = {} as this & { _foo: 'bar' };
    that.prop = 1;
  }
}
    
}
namespace Case662 {

class TestUnion {
  private prop: number = 3;

  test() {
    const that = {} as this | (this & { _foo: 'bar' });
    that.prop = 1;
  }
}
    
}
namespace Case2359 {

class Test {
  private readonly prop: number;

  constructor() {
    const that = {} as this & { _foo: 'bar' };
    that.prop = 1;
  }
}
      
}
