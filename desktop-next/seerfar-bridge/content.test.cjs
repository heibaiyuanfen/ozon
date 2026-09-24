const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { test } = require('node:test');
const vm = require('node:vm');

const script = readFileSync(join(__dirname, 'content.js'), 'utf8');

function runPanel(panel, { depth = 0, bodyText = '' } = {}) {
  let sent;
  let observerOptions;
  let badge;
  const panelElement = { innerText: panel, parentElement: null };
  let labelParent = panelElement;
  for (let i = 0; i < depth; i++) labelParent = { innerText: '卖家实际售价', parentElement: labelParent };
  const labelNode = { textContent: '卖家实际售价', parentElement: labelParent };
  class Observer {
    observe(_element, options) { observerOptions = options; }
    disconnect() {}
  }
  const context = {
    location: { hash: '#ozon-erp-capture=12345.0123456789abcdef0123456789abcdef', pathname: '/product/example-4983882608/', href: 'https://www.ozon.ru/product/example-4983882608/' },
    document: {
      body: { innerText: bodyText, appendChild(element) { badge = element; } }, documentElement: {},
      createElement() { return { textContent: '', style: {}, setAttribute() {} }; },
      createTreeWalker() { let done = false; return { nextNode() { if (done) return null; done = true; return labelNode; } }; },
      querySelector() { return null; },
    },
    NodeFilter: { SHOW_TEXT: 4 },
    MutationObserver: Observer,
    chrome: { runtime: { async sendMessage(message) { sent = message; return { ok: true }; } } },
    setInterval() { return 1; }, clearInterval() {}, setTimeout() { return 1; }, clearTimeout() {},
    console: { info() {}, warn() {} }, Date,
  };
  vm.runInNewContext(script, context);
  return new Promise(resolve => setImmediate(() => resolve({ sent, observerOptions, badge })));
}

test('captures visible Seerfar fields with a full-width SKU colon', async () => {
  const { sent, observerOptions, badge } = await runPanel('SKU：4983882608\n卖家实际售价：248 ₽\n重量：65 g\n体积：150×150×20mm\n库存：749');
  assert.equal(sent?.kind, 'ozon-erp-seerfar-capture');
  assert.equal(sent.payload.sellerPriceRub, 248);
  assert.equal(sent.payload.weightG, 65);
  assert.equal(sent.payload.stock, 749);
  assert.equal(sent.payload.dimensionsMm, '150×150×20mm');
  assert.equal(observerOptions.characterData, true);
  assert.match(badge.textContent, /已回填到 ERP/);
});

test('does not send a different SKU to the local receiver', async () => {
  const { sent, badge } = await runPanel('SKU: 1111111111\n卖家实际售价: 248 ₽');
  assert.equal(sent, undefined);
  assert.match(badge.textContent, /不一致/);
});

test('finds a panel beyond the old nine-ancestor limit', async () => {
  const { sent } = await runPanel('SKU: 4983882608\n卖家实际售价: 441 ₽', { depth: 12 });
  assert.equal(sent?.payload.sellerPriceRub, 441);
});

test('reads visible body text when labels live in separate DOM branches', async () => {
  const panel = 'SKU: 4983882608\n卖家实际售价: 441 ₽';
  const { sent } = await runPanel('卖家实际售价: 441 ₽', { bodyText: panel });
  assert.equal(sent?.payload.sellerPriceRub, 441);
});

test('captures a SKU separated by invisible characters and line breaks', async () => {
  const { sent } = await runPanel('SKU：\u200b\n4983882608\n卖家实际售价：441 ₽');
  assert.equal(sent?.payload.sellerPriceRub, 441);
});

test('keeps a mismatched panel from reaching the receiver even with the page SKU visible', async () => {
  const { sent } = await runPanel('SKU: 1111111111\n卖家实际售价: 441 ₽', {
    bodyText: '4983882608\nSKU: 1111111111\n卖家实际售价: 441 ₽',
  });
  assert.equal(sent, undefined);
});
