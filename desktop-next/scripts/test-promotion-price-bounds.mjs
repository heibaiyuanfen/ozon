import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/promotionPriceBounds.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { promotionPriceBounds } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);

test("AUTO-2 may submit 19 -> 14.10 when only the minimum is unpublished", () => {
  for (const minimum of [0, undefined, null]) {
    const bounds = promotionPriceBounds([{ minActionPrice: minimum, maxActionPrice: 18 }]);
    assert.ok(14.10 >= bounds.lower && 14.10 <= bounds.upper);
    assert.equal(bounds.missingMinimum, true);
  }
});

test("an unpublished minimum in one activity preserves other activities' known lower limits", () => {
  const bounds = promotionPriceBounds([{ minActionPrice: 0, maxActionPrice: 18 }, { minActionPrice: 15, maxActionPrice: 17 }]);
  assert.ok(14.10 < bounds.lower);
  assert.equal(bounds.upper, 17);
  assert.ok(15 >= bounds.lower && 15 <= bounds.upper);
});

test("each activity must supply its upper limit before a promotion update can be planned", () => {
  for (const maximum of [0, undefined, NaN, Infinity]) {
    assert.equal(promotionPriceBounds([{ minActionPrice: 0, maxActionPrice: 18 }, { minActionPrice: 0, maxActionPrice: maximum }]).upper, null);
  }
});

test("known lower and upper limits include their exact boundaries", () => {
  const bounds = promotionPriceBounds([{ minActionPrice: 14.10, maxActionPrice: 19 }]);
  assert.equal(bounds.lower, 14.10);
  assert.equal(bounds.upper, 19);
  assert.equal(bounds.missingMinimum, false);
  assert.ok(14.09 < bounds.lower);
  assert.ok(19.01 > bounds.upper);
});
