namespace Case3057 {

class Test {
  private readonly prop = { foo: 'bar' };
}
      
}
namespace Case3137 {

class Test {
  private readonly prop = [1, 2, 'three'];

  constructor() {
    this.prop = [1, 2, 'four'];
  }
}
      
}
namespace Case3361 {

declare const hello: 'hello' | 10;

class Test {
  private readonly prop = hello;

  constructor() {
    this.prop = 10;
  }
}
      
}
namespace Case3477 {

class Test {
  private readonly prop = Promise.resolve('hello');
}
      
}
