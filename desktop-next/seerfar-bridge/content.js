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

  // A visible, ticket-scoped status makes it possible to distinguish a missing
  // content script from a panel parse or loopback delivery failure.
  const badge = document.createElement("div");
  badge.setAttribute("role", "status");
  badge.style.cssText = "position:fixed;top:12px;right:12px;z-index:2147483647;max-width:360px;padding:10px 14px;border-radius:8px;background:#14213d;color:#fff;font:13px/1.5 sans-serif;box-shadow:0 4px 18px #0004;pointer-events:none";
  document.body.appendChild(badge);
  const status = message => { badge.textContent = `Ozon ERP · ${message}`; };
  status("已识别采集链接，等待 Seerfar 面板…");

  const normalize = text => String(text || "").replace(/[\u200b-\u200f\u2060\ufeff]/g, "");
  const hasProductPanel = text => {
    const visible = normalize(text);
    return visible.includes("卖家实际售价") && new RegExp(`SKU[\\s:：]*${sku}(?!\\d)`, "i").test(visible);
  };

  function panelText(root) {
    // The Seerfar price and SKU can be separated by many nested elements.
    // innerText contains only visible light-DOM text and avoids an arbitrary
    // ancestor-depth limit when both fields are in the same document.
    const visibleText = normalize(root.innerText || root.textContent || "");
    if (visibleText.length <= 200000 && hasProductPanel(visibleText)) return visibleText;
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let node;
    let seen = 0;
    let otherPanel = "";
    while ((node = walker.nextNode()) && seen++ < 150000) {
      if (!node.textContent?.includes("卖家实际售价")) continue;
      let element = node.parentElement;
      for (let depth = 0; element && depth < 24; depth++, element = element.parentElement) {
        const text = normalize(element.innerText || element.textContent || "");
        if (text.length <= 50000 && hasProductPanel(text)) return text;
        if (!otherPanel && text.length <= 50000 && /SKU\s*[:：]/i.test(text) && text.includes("卖家实际售价"))
          otherPanel = text;
      }
    }
    // Seerfar may use an open shadow root; closed roots and iframes remain inaccessible.
    for (const element of root.querySelectorAll?.("*") || []) {
      if (element.shadowRoot) {
        const text = panelText(element.shadowRoot);
        if (text) return text;
      }
    }
    // Some overlays render the label and value in separate DOM branches. A
    // page-wide fallback is safe only when the current SKU is visible too.
    if (root === document.body && visibleText.includes("卖家实际售价") &&
        new RegExp(`(?:^|\\D)${sku}(?!\\d)`).test(visibleText)) return visibleText;
    return otherPanel;
  }

  const field = (text, label) => {
    const match = new RegExp(`${label}\\s*[:：]?\\s*([^\\n\\r]+)`, "i").exec(normalize(text));
    return match?.[1]?.trim() || "";
  };
  const number = value => {
    const match = value.replace(/\s/g, "").match(/-?\d+(?:[.,]\d+)?/);
    return match ? Number(match[0].replace(",", ".")) : null;
  };
  let busy = false;
  let completed = false;
  let observer;
  let lastError = "";
  const started = Date.now();
  const timeoutMessage = () => {
    if (lastError) return `采集超时：${lastError}`;
    if ((document.body.innerText || "").includes("卖家实际售价"))
      return `采集超时：找到售价标签，但未找到匹配的商品 SKU ${sku}`;
    return "采集超时：售价标签不在可读取的页面 DOM 中；面板可能位于隔离 iframe 或封闭 Shadow DOM";
  };
  async function tryCapture() {
    if (busy || completed || Date.now() - started > 90000) {
      if (Date.now() - started > 90000) {
        observer?.disconnect();
        status(timeoutMessage());
      }
      return;
    }
    const text = panelText(document.body);
    if (!text) return;
    const panelSku = new RegExp(`SKU[\\s:：]*(${sku})(?!\\d)`, "i").exec(normalize(text))?.[1];
    const price = number(field(text, "卖家实际售价"));
    if (panelSku !== sku) {
      lastError = `面板 SKU ${panelSku || "未识别"} 与商品 ${sku} 不一致`;
      status(`面板 SKU ${panelSku || "未识别"} 与商品 ${sku} 不一致`);
      return;
    }
    if (price === null) {
      lastError = "已找到面板，但未识别到卖家实际售价";
      status("已找到面板，但未识别到卖家实际售价");
      return;
    }
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
    status("已读取面板，正在回填到 ERP…");
    try {
      const response = await chrome.runtime.sendMessage({ kind: "ozon-erp-seerfar-capture", port, payload });
      if (response?.ok) {
        completed = true;
        observer?.disconnect();
        status(`SKU ${sku} 已回填到 ERP`);
        console.info("Ozon ERP: Seerfar visible fields saved locally for SKU", sku);
      } else {
        lastError = `本机回填失败：${String(response?.error || "未知错误").slice(0, 160)}`;
        status(`本机回填失败：${String(response?.error || "未知错误").slice(0, 160)}`);
        console.warn("Ozon ERP: local capture failed", response?.error);
      }
    } catch (error) {
      lastError = `本机回填失败：${String(error).slice(0, 160)}`;
      status(`本机回填失败：${String(error).slice(0, 160)}`);
      console.warn("Ozon ERP: local capture failed", error);
    } finally {
      busy = false;
    }
  }
  observer = new MutationObserver(() => {
    clearTimeout(observer.timer);
    observer.timer = setTimeout(tryCapture, 600);
  });
  observer.observe(document.documentElement, { childList: true, characterData: true, subtree: true });
  void tryCapture();
  // Some Seerfar panels render before filling their text, or inside an open
  // shadow root whose mutations are not observed by the document observer.
  const retry = setInterval(() => {
    if (completed || Date.now() - started > 90000) {
      clearInterval(retry);
      observer?.disconnect();
      if (!completed) status(timeoutMessage());
    } else {
      void tryCapture();
    }
  }, 2000);
  setTimeout(() => { clearInterval(retry); observer?.disconnect(); }, 91000);
})();
