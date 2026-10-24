interface Item { hasTag(tag: string): boolean }
interface Options { displayItem: (item: Item) => boolean }
export const options: Options = { displayItem: item => item.hasTag("a") };

export const inline: { show: (row: { id: number }) => void } = { show: row => row.id };
