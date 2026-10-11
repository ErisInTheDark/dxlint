declare function v(): void;
declare namespace JSX { interface IntrinsicElements { button: { onClick: () => void }; "X-widget": { onClick: () => void } } }
declare const button: () => number;
declare function Component(props: { onClick: () => number }): unknown;
const intrinsic = <button onClick={() => v()} />;
const customTag = <X-widget onClick={() => v()} />;
const component = <Component onClick={() => { v(); }} />;
const unknownTag = <missing onClick={() => { v(); }} />;
