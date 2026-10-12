declare const cards: {name: {match(expression: any): string[] | null}}[];
declare function schedule(callback: (section: {dataset: {sectionId: string}}) => void): void;
declare function nullableArray(): string[] | null;

for (const card of cards) {
  const mod = card.name.match(/^\s*\[(.*?)\]/);
  if (mod) {
    schedule(section => {
      section.dataset.sectionId = mod[1];
    });
  }
}

declare const nullable: string[] | null;
if (nullable) {
  const arrow = () => {
    const unrelated = "nullable = null";
    const fromArrow = nullable[0];
  };
  const ordinary = function () {
    const fromFunction = nullable[0];
  };
  const nested = () => () => {
    const fromNested = nullable[0];
  };
  const deferred = async () => {
    await 0;
    const fromAsync = nullable[0];
  };
  const parameter = (nullable: string[] | null) => {
    const fromParameter = nullable[0];
  };
  {
    const nullable: string[] | null = nullableArray();
    const fromBlockShadow = nullable[0];
    schedule(section => {
      section.dataset.sectionId = nullable[0];
    });
  }
}

function create() {
  const value = nullableArray();
  if (!value) return;
  return () => {
    const fromEarlyExit = value[0];
  };
}

let mutable = nullableArray();
if (mutable) {
  schedule(section => {
    section.dataset.sectionId = mutable[0];
  });
}

const written: string[] | null = nullableArray();
if (written) {
  schedule(section => {
    section.dataset.sectionId = written[0];
  });
}
written = null;

const object = {value: nullableArray()};
if (object.value) {
  schedule(section => {
    section.dataset.sectionId = object.value[0];
  });
}
