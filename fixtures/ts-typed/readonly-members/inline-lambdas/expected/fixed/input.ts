class Owner {
  private number = 7;
  #number = 7;
  private readonly arrow = () => 7;
  private wrapped = (() => 7);
  private regular = function () { return 7; };
  private readonly unset: number;
  constructor(private readonly parameter: number, private supplied = 7, private readonly callback = () => 7) {}
}
