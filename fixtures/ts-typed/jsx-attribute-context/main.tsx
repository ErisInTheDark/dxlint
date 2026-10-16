declare function Widget(props: { cb: () => void; render: () => number }): unknown;
declare const Missing: unknown;
declare function Empty(): unknown;
const known = <Widget cb={() => 102} />;
const unknown = <Missing cb={() => 103} />;
const spread = <Widget {...(() => 104)} />;
const empty = <Empty cb={() => 105} />;
const numeric = <Widget render={() => 106} />;
