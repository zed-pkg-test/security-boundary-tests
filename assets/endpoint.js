export function validateApiPath(value) {
  if (typeof value !== "string" ||
      !/^\/[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*\/?$/.test(value) ||
      value.length > 128) {
    throw new Error("API endpoint must be a same-origin canonical absolute path");
  }
  return value.replace(/\/$/, "");
}
export function normalizeList(payload) {
  const items = Array.isArray(payload) ? payload : payload?.items;
  if (!Array.isArray(items) || items.length > 10000 ||
      !items.every(item => item && typeof item === "object" &&
        !Array.isArray(item) && typeof item.metadata?.name === "string")) {
    throw new Error("Invalid resource list or too many resources");
  }
  return items;
}
export function safeQuantity(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value : 0;
}
