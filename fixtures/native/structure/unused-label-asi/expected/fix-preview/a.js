function brackets() { call()
  label: [1].forEach(work);
}
function parens() { call()
  label: (function () {})();
}
function regex() { call()
  label: /x/.test(value);
}
function plus() { call()
  label: +value;
}
function minus() { call()
  label: -value;
}
function template() { call()
  label: `x${value}` + other;
}
function nested() { call()
  inner: [1].forEach(work);
}
function safe() { call();
  [1].forEach(work);
}
