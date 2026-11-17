// Code 128 symbol widths, alternating bars and spaces (including start and stop).
const widths = "212222 222122 222221 121223 121322 131222 122213 122312 132212 221213 221312 231212 112232 122132 122231 113222 123122 123221 223211 221132 221231 213212 223112 312131 311222 321122 321221 312212 322112 322211 212123 212321 232121 111323 131123 131321 112313 132113 132311 211313 231113 231311 112133 112331 132131 113123 113321 133121 313121 211331 231131 213113 213311 213131 311123 311321 331121 312113 312311 332111 314111 221411 431111 111224 111422 121124 121421 141122 141221 112214 112412 122114 122411 142112 142211 241211 221114 413111 241112 134111 111242 121142 121241 114212 124112 124211 411212 421112 421211 212141 214121 412121 111143 111341 131141 114113 114311 411113 411311 113141 114131 311141 411131 211412 211214 211232 2331112".split(" ");

export function code128Symbols(value: string): number[] {
  if (!value || /[^ -~]/.test(value)) throw new Error("条码必须由可打印 ASCII 字符组成");
  const numeric = /^(?:\d{2})+$/.test(value);
  const data = numeric ? value.match(/../g)!.map(Number) : Array.from(value, char => char.charCodeAt(0) - 32);
  const start = numeric ? 105 : 104;
  const checksum = data.reduce((sum, symbol, index) => sum + symbol * (index + 1), start) % 103;
  return [start, ...data, checksum, 106];
}

type ProductBarcode = { sku: string; offerId: string; barcodes: string[] };
const xml = (s: string) => s.replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&apos;"})[c]!);

export function productBarcodeSvg(products: ProductBarcode[]) {
  const labels = [];
  let skipped = 0;
  for (const product of products) {
    const codes = [...new Set(product.barcodes)];
    if (!codes.length) skipped++;
    for (const code of codes) {
      if (!code || /[^ -~]/.test(code)) { skipped++; continue; }
      let x = 10; // Ten modules of quiet zone on each side.
      const bars: string[] = [];
      for (const symbol of code128Symbols(code)) {
        Array.from(widths[symbol]).forEach((width, index) => {
          if (index % 2 === 0) bars.push(`<rect x="${x}" y="0" width="${width}" height="45"/>`);
          x += Number(width);
        });
      }
      labels.push({ code, offer: product.offerId || product.sku, bars, width: x + 10 });
    }
  }
  if (!labels.length) throw new Error("没有可下载的条码，请先获取产品条形码");
  const width = Math.max(174, ...labels.map(label => label.width));
  const height = labels.length * 96;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${(width / 3).toFixed(2)}mm" height="${height / 3}mm" viewBox="0 0 ${width} ${height}"><rect width="100%" height="100%" fill="white"/>` +
    labels.map((label, index) => `<g transform="translate(0 ${index * 96})"><text x="${width / 2}" y="14" text-anchor="middle" font-family="sans-serif" font-size="11">${xml(label.offer.slice(0, 25))}</text><g transform="translate(${(width - label.width) / 2} 22)" shape-rendering="crispEdges">${label.bars.join("")}</g><text x="${width / 2}" y="82" text-anchor="middle" font-family="monospace" font-size="11">${xml(label.code)}</text></g>`).join("") + "</svg>";
  return { svg, count: labels.length, skipped };
}

export function downloadProductBarcodeLabels(products: ProductBarcode[]) {
  const result = productBarcodeSvg(products);
  const url = URL.createObjectURL(new Blob([result.svg], { type: "image/svg+xml;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  const name = products.length === 1 ? (products[0].offerId || products[0].sku).replace(/[^a-zA-Z0-9_-]/g, "_") : "selected";
  link.download = `Ozon-barcode-${name}.svg`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 30_000);
  return result;
}

