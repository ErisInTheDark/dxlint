namespace Case474 {

class Foo {
  private value: number = 0;

  bar(newValue: { value: number }) {
    ({ value: this.value } = newValue);
    return this.value;
  }
}
      
}
namespace Case486 {

class Foo {
  #value: number = 0;

  bar(newValue: { value: number }) {
    ({ value: this.#value } = newValue);
    return this.#value;
  }
}
      
}
namespace Case524 {

class Foo {
  private value: Record<string, number> = {};

  bar(newValue: Record<string, number>) {
    ({ ...this.value } = newValue);
    return this.value;
  }
}
      
}
namespace Case548 {

class Foo {
  private value: number[] = [];

  bar(newValue: number[]) {
    [...this.value] = newValue;
    return this.value;
  }
}
      
}
