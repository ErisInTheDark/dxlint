declare const external: unknown;
class Owner {
  private value = 1;
  private stable = 1;
  update() { external.value = 2; }
}
