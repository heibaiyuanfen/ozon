export const wbCostFields = [
  ["purchase","单件商品成本","CNY"], ["firstMile","单件头程运费","CNY"],
  ["length","长","cm"], ["width","宽","cm"], ["height","高","cm"], ["weight","重量","kg"],
] as const;
export type WbProductCost = Partial<Record<typeof wbCostFields[number][0], string>>;
export type WbWarehouseCost = { kind: "domestic" | "overseas"; handling: string };

export function readWbData<T>(key: string): Record<string,T> {
  try { return JSON.parse(localStorage.getItem(key) ?? "{}") ?? {}; } catch { return {}; }
}
export const wbProductCostKey = (shop: string) => `wb-product-cost-v1:${shop}`;
export const wbWarehouseCostKey = (shop: string) => `wb-warehouse-cost-v1:${shop}`;
