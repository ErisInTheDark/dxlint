namespace Case469 {

class TestComputedParameter {
  private ['computed-ignored-by-rule'] = 1;
}
    
}
namespace Case737 {

class Test1 {
  // should not report
  private prop = 7;

  foo() {
    this['prop'] = 10;
  }
}

class Test3 {
  // should not report
  private 'prop' = 7;

  foo() {
    this.prop = 10;
  }
}
    
}
namespace Case758 {

class Test2 {
  // should report
  private [Symbol.iterator] = 7;
}

class Test4 {
  // should report
  private [1] = 7;
}
      
}
