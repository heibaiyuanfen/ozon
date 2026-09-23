// This reads page-visible DOM only. Chrome does not allow one extension to read
// another extension's private storage, closed shadow root or cross-origin iframe.
(() => {
  const ticket = /^#ozon-erp-capture=(\d{4,5})\.([a-f0-9]{32})$/i.exec(location.hash);
  if (!ticket) return;
  const port = Number(ticket[1]);
  const token = ticket[2];
  const product = /\/product\/(?:[^/?#]+-)?(\d{7,})\/?/i.exec(location.pathname);
  if (!product) return;
  const sku = product[1];

  function panelText(root) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let node;
    let seen = 0;
    while ((node = walker.nextNode()) && seen++ < 150000) {
      if (!node.textContent?.includes("卖家实际售价")) continue;
      let element = node.parentElement;
      for (let depth = 0; element && depth < 9; depth++, element = element.parentElement) {
        const text = element.innerText || "";
        if (text.includes("SKU:") && text.includes("卖家实际售价") && text.length < 5000) return text;
      }
    }
    // Seerfar may use an open shadow root; closed roots and iframes remain inaccessible.
    for (const element of root.querySelectorAll?.("*") || []) {
      if (element.shadowRoot) {
        const text = panelText(element.shadowRoot);
        if (text) return text;
      }
    }
    return "";
  }

  const field = (text, label) => {
    const match = new RegExp(`${label}\\s*[:：]\\s*([^\\n\\r]+)`, "i").exec(text);
    return match?.[1]?.trim() || "";
  };
  const number = value => {
    const match = value.replace(/\s/g, "").match(/-?\d+(?:[.,]\d+)?/);
    return match ? Number(match[0].replace(",", ".")) : null;
  };
  let busy = false;
  let completed = false;
  let observer;
  const started = Date.now();
  async function tryCapture() {
    if (busy || completed || Date.now() - started > 60000) {
      if (Date.now() - started > 60000) observer?.disconnect();
      return;
    }
    const text = panelText(document.body);
    if (!text) return;
    const panelSku = field(text, "SKU").match(/\d{7,}/)?.[0];
    const price = number(field(text, "卖家实际售价"));
    if (panelSku !== sku || price === null) return;
    const image = document.querySelector('meta[property="og:image"]')?.content || "";
    const payload = {
      token, sku, url: location.href,
      sellerPriceRub: price,
      weightG: number(field(text, "重量")),
      dimensionsMm: field(text, "体积"),
      category: field(text, "类目"),
      stock: number(field(text, "库存")),
      seller: field(text, "卖家"),
      listingDate: field(text, "上架时间").match(/\d{4}-\d{2}-\d{2}/)?.[0] || "",
      imageUrl: image,
    };
    busy = true;
    try {
      const response = await chrome.runtime.sendMessage({ kind: "ozon-erp-seerfar-capture", port, payload });
      if (response?.ok) {
        completed = true;
        observer?.disconnect();
        console.info("Ozon ERP: Seerfar visible fields saved locally for SKU", sku);
      } else {
        console.warn("Ozon ERP: local capture failed", response?.error);
      }
    } catch (error) {
      console.warn("Ozon ERP: local capture failed", error);
    } finally {
      busy = false;
    }
  }
  observer = new MutationObserver(() => {
    clearTimeout(observer.timer);
    observer.timer = setTimeout(tryCapture, 600);
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  void tryCapture();
  setTimeout(() => observer?.disconnect(), 61000);
})();
