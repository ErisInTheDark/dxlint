interface Later<T> {
  recover<R = never>(handler?: ((reason: any) => R | Later<R>) | undefined | null): Later<T | R>;
}
declare const later: Later<number>;
later.recover(error => error);
