declare function use(cb: () => void): void;
use(function* () { yield 1; });
use(async function* () { yield 1; });
declare const values: number[];
use(function* () { yield* values; });

function* outer() {
  function* inner() { yield false; }
  yield true;
  return true;
}
if (outer().next().value) {}

declare const truth: Promise<true>;
async function* asyncOuter() { yield truth; return truth; }
async function check() { if ((await asyncOuter().next()).value) {} }

function* annotated(): Generator<boolean, boolean, unknown> { yield true; return true; }
if (annotated().next().value) {}

use(async function* () { yield* values; });

function* bareYield() { yield; return true; }
if (bareYield().next().value) {}
function* fallsThrough() { yield true; }
if (fallsThrough().next().value) {}
async function* asyncBareYield() { yield; return true; }
async function* asyncFallsThrough() { yield truth; }
async function checkOptional() {
  if ((await asyncBareYield().next()).value) {}
  if ((await asyncFallsThrough().next()).value) {}
}
