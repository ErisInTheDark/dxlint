class Owner {
  private number = 7;
  #number = 7;
  private arrow = () => 7;
  private wrapped = (() => 7);
  private regular = function () { return 7; };
  private unset: number;
  constructor(private parameter: number, private supplied = 7, private callback = () => 7) {}
}
