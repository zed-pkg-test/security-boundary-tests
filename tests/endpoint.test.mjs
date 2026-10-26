import test from "node:test";
import assert from "node:assert/strict";
import { validateApiPath, normalizeList, safeQuantity } from "../assets/endpoint.js";
test("canonical same-origin paths only", () => {
  assert.equal(validateApiPath("/api/v1"), "/api/v1");
  for (const v of ["https://evil.example", "//evil.example", "/api/../admin",
    "/api?token=x", "/api#frag", "/a//b", "/a%2fb", "/a\\b", "", "/"]) {
    assert.throws(() => validateApiPath(v));
  }
});
test("bounded structured list", () => {
  const item = {metadata: {name: "a"}};
  assert.deepEqual(normalizeList({items: [item]}), [item]);
  assert.throws(() => normalizeList({items: [null]}));
  assert.throws(() => normalizeList({items: Array(10001).fill(item)}));
  assert.throws(() => normalizeList({error: "unauthorized"}));
});
test("untrusted quantities never become NaN", () => {
  assert.equal(safeQuantity(100), 100);
  for (const v of ["100", NaN, -1, Infinity, Number.MAX_SAFE_INTEGER + 1, true])
    assert.equal(safeQuantity(v), 0);
});
