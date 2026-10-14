class Hash {
  #object;
  #array;
  #rest;
  #read = 0;
  #updated = 0;
  #nested = { value: 0 };
  get #accessor() {}
  set #accessor(value) {}
  method(other) {
    ({ value: this.#object } = input);
    [this.#array] = input;
    [...this.#rest] = input;
    consume(other.#read);
    consume(this.#updated++);
    this.#nested.value = 1;
    this.#accessor = 1;
  }
}
class Counters {
  #discarded = 0;
  #assigned = 0;
  #observed = 0;
  method() {
    this.#discarded++;
    this.#discarded += 1;
    consume(this.#assigned = 1);
    consume(this.#observed += 1);
  }
}
class Shadow {
  #shadow;
  method() {
    class Inner {
      #shadow;
      read() { return this.#shadow; }
    }
    return Inner;
  }
}
class UnusedAccessor {
  get #value() {}
  set #value(value) {}
}
