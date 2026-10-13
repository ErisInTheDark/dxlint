declare namespace JSX {
  interface IntrinsicElements { div: { key?: string | number }; }
}
declare const props: { id?: string | number }, known: string;
declare function side(): void;
const attribute = <div key={props.id!} />;
const child = <div>{known!}</div>;
const sequence = <div key={(side(), known)} />;
