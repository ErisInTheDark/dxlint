enum Fruit { Apple }
declare let fruit: Fruit | undefined;
fruit ??= 1;
fruit ||= 1;
fruit &&= 1;
declare let nullable: Fruit | null;
nullable ??= 1;
declare let ordinary: Fruit | number;
ordinary ??= 1;
declare let dynamic: Fruit | unknown;
dynamic ??= 1;
declare let safe: Fruit | undefined;
safe ??= Fruit.Apple;
