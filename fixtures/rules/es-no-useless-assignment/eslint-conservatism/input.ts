function overwrittenBeforeFinally() {
  let outcome = "unknown";
  try {
    helper1();
    outcome = "success";
  } catch (error) {
    outcome = "exception";
    helper2();
  } finally {
    console.log(outcome);
  }
}

function* stoppedAtYield() {
  let done = false;
  try {
    yield 1;
    done = true;
  } catch {
    done = true;
  } finally {
    if (!done) console.log("stopped");
  }
}
