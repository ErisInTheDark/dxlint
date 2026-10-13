declare function task(): Promise<void>;
declare const uncertain: any;
false && task();
true || task();
false ?? task();
0 ?? task();
"" ?? task();
({}) ?? task();
true && task();
false || task();
null ?? task();
undefined ?? task();
uncertain && task();
uncertain || task();
uncertain ?? task();
task() || task();
async function conditionalFlow() {
  let condition = false;
  condition && task();
  condition = true;
  condition || task();
  condition ?? task();
}
