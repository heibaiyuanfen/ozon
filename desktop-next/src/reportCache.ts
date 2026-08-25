const clearers = new Set<() => void>();

export function registerReportCache(clearer: () => void) {
  clearers.add(clearer);
  return () => clearers.delete(clearer);
}

export function clearReportCache() {
  clearers.forEach((clearer) => clearer());
}

window.addEventListener("report-settings-changed", clearReportCache);
