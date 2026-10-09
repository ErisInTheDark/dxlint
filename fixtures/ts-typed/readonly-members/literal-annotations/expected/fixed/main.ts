namespace Case2393 {

class Test {
  private readonly prop: string = 'hello';

  constructor() {
    this.prop = 'world';
  }
}
      
}
namespace Case2425 {

class Test {
  private readonly prop = 'hello';
}
      
}
namespace Case2449 {

declare const hello: 'hello';

class Test {
  private readonly prop = hello;

  constructor() {
    this.prop = 'world';
  }
}
      
}
namespace Case2513 {

class Test {
  private readonly prop: number = 10;

  constructor() {
    this.prop = 11;
  }
}
      
}
namespace Case2605 {

class Test {
  private readonly prop: boolean = true;

  constructor() {
    this.prop = false;
  }
}
      
}
