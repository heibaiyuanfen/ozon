export interface PromotionPriceLimit {
  minActionPrice: number;
  maxActionPrice: number;
}

// A missing/zero minimum is not an explicit prohibition on lowering a price.
// Keep every known lower bound; the platform validates any unpublished limit.
export function promotionPriceBounds(products: PromotionPriceLimit[]) {
  const knownMinimums = products.map((p) => p.minActionPrice).filter((v) => Number.isFinite(v) && v > 0);
  return {
    lower: Math.max(0, ...knownMinimums),
    upper: products.length > 0 && products.every((p) => Number.isFinite(p.maxActionPrice) && p.maxActionPrice > 0)
      ? Math.min(...products.map((p) => p.maxActionPrice)) : null,
    missingMinimum: knownMinimums.length < products.length,
  };
}
