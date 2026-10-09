namespace Case2697 {

enum Foo {
  Bar,
  Bazz,
}

class Test {
  private readonly prop: Foo = Foo.Bar;

  constructor() {
    this.prop = Foo.Bazz;
  }
}
      
}
namespace Case2773 {

enum Foo {
  Bar,
  Bazz,
}

const foo = Foo.Bar;

class Test {
  private readonly prop: Foo = foo;

  constructor() {
    this.prop = foo;
  }
}
      
}
namespace Case2895 {

enum Foo {
  Bar,
  Bazz,
}

const bar = Foo.Bar;

function wrapper() {
  const Foo = 10;

  class Test {
    private prop = bar;

    constructor() {
      this.prop = bar;
    }
  }
}
      
}
namespace Case2949 {

enum Foo {
  Bar,
  Bazz,
}

const bar = Foo.Bar;

function wrapper() {
  type Foo = 10;

  class Test {
    private prop = bar;

    constructor() {
      this.prop = bar;
    }
  }
}
      
}
namespace Case3003 {

const Bar = (function () {
  enum Foo {
    Bar,
    Bazz,
  }

  return Foo;
})();

const bar = Bar.Bar;

class Test {
  private prop = bar;

  constructor() {
    this.prop = bar;
  }
}
      
}
