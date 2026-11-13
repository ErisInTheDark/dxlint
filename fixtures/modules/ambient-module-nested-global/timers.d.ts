declare module "timers" {
  function onlyInTheModule(ms: number): number;
  global {
    function setTimer(ms: number): number;
  }
}
