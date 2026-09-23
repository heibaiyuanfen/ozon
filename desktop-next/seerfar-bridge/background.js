chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.kind !== "ozon-erp-seerfar-capture") return;
  const { port, payload } = message;
  if (!Number.isInteger(port) || port < 1024 || port > 65535) {
    sendResponse({ ok: false, error: "invalid port" });
    return;
  }
  fetch(`http://127.0.0.1:${port}/capture`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  }).then(async response => sendResponse({ ok: response.ok, error: response.ok ? "" : await response.text() }))
    .catch(error => sendResponse({ ok: false, error: String(error) }));
  return true;
});
