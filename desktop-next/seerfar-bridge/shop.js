// Reads only rendered product cards. Never calls private Seerfar APIs.
(() => {
  if (!location.hash.startsWith('#ozon-erp-shop')) return;
  const store = location.pathname.match(/^\/seller\/[^/]+/)?.[0];
  if (!store) return;
  const key = 'ozon-erp-shop:' + store;
  let saved;
  try { saved = JSON.parse(sessionStorage.getItem(key) || '{}'); } catch { saved = {}; }
  const rows = new Map((saved.rows || []).map(r => [r.SKU, r]));
  const pages = new Set(saved.pages || []);
  let running = false;
  let auto = saved.auto === true;
  if (document.querySelector?.('[data-ozon-erp-shop]')) return;
  const panel = document.createElement('div');
  panel.dataset.ozonErpShop='true';
  panel.style.cssText = 'position:fixed;right:12px;top:12px;z-index:2147483647;width:330px;padding:14px;background:#14213d;color:white;border-radius:10px;font:13px/1.6 sans-serif';
  const info = document.createElement('div'); panel.append(info);
  // The seller header may load after this content script and may not use h1.
  // The Ozon seller page title supplies a fallback only in the known store-title format.
  function currentShopName() {
    const heading = document.querySelector?.('h1')?.innerText?.trim();
    if (heading) return normalizeName(heading);
    const title = String(document.title || '');
    const match = title.match(/^(.+?)\s*[—–-]{2}\s*(?:OZON|ОЗОН)/i);
    return normalizeName(match?.[1] || '');
  }
  function normalizeName(value) { return String(value || '').replace(/[\u200b-\u200f\u2060\ufeff]/g, '').replace(/(?:本土|跨境)\s*$/, '').replace(/\s+/g, ' ').trim().toLocaleLowerCase(); }
  const verified = row => { const name=currentShopName(); return !!name && !!row['卖家'] && normalizeName(row['卖家']) === name; };
  const show = text => { const count = [...rows.values()].filter(verified).length; info.textContent = `整店采集 ${store} · 归属已核对 ${count} · 待核对 ${rows.size-count} · ${pages.size} 页。${currentShopName() ? '' : '未识别店铺名称，无法确认商品归属。'}${text}`; };
  const persist = () => {
    try { sessionStorage.setItem(key, JSON.stringify({rows:[...rows.values()],pages:[...pages],auto})); }
    catch { show('浏览器存储已满，请立即导出备份。'); running = false; }
  };
  const field = (text, label) => {
    const labels = '近30天销售额|近30天销量|广告费用份额|产品卡浏览量|退货取消率|重量|体积|类目|库存|品牌|卖家|配送|变体数|跟卖数|毛利率|曝光量|加购率|上架时间';
    const match = new RegExp(label + '\\s*[:：]?\\s*([^\\n\\r]+)', 'i').exec(text);
    return (match?.[1] || '').split(new RegExp('\\s*(?:' + labels + ')\\s*[:：]?'))[0].trim();
  };
  const normalize = text => String(text || '').replace(/[\u200b-\u200f\u2060\ufeff]/g, '');
  function skuPanels() {
    const result = new Map();
    // Seed from the panel's own SKU label, independent of product-link ancestry.
    for (const seed of document.querySelectorAll('body *')) {
      if (panel.contains?.(seed)) continue;
      const label = normalize(seed.textContent || '');
      if (label.length > 180) continue;
      const sku = /SKU[\s:：]*(\d{7,})(?!\d)/i.exec(label)?.[1];
      if (!sku) continue;
      let element = seed;
      for (let depth=0; element && element !== document.body && depth<24; depth++,element=element.parentElement) {
        const text = normalize(element.innerText || '');
        const ids = [...text.matchAll(/SKU[\s:：]*(\d{7,})(?!\d)/gi)].map(m=>m[1]);
        if (ids.some(id=>id!==sku)) break;
        if (ids.includes(sku) && /重量|体积|类目/.test(text)) {
          const old = result.get(sku) || '';
          if (text.length > old.length && text.length < 50000) result.set(sku,text);
        }
      }
    }
    return result;
  }
  function recommendationBoundary() {
    // Only a distinct heading is evidence; product titles and plugin text are not.
    const pattern = /^(推荐商品|为您推荐|猜你喜欢|您可能还喜欢|推荐给您|рекомендуем вам|рекомендации для вас|вам может понравиться|рекомендуемые товары)$/i;
    const candidates = [...document.querySelectorAll('h1,h2,h3,h4,[role=heading]')];
    return candidates.find(el => pattern.test((el.innerText || '').trim()) && el.getClientRects().length) || null;
  }
  function capture() {
    let found = 0;
    const panels = skuPanels();
    const boundary = recommendationBoundary();
    for (const a of document.querySelectorAll('a[href*="/product/"]')) {
      if (boundary && (boundary.compareDocumentPosition(a) & Node.DOCUMENT_POSITION_FOLLOWING)) continue;
      const url = new URL(a.href, location.href);
      if (!['ozon.ru','www.ozon.ru'].includes(url.hostname)) continue;
      const sku = /\/product\/(?:[^/?#]*-)?(\d{7,})\/?/.exec(url.pathname)?.[1];
      if (!sku) continue;
      // Stop at the smallest ancestor containing this SKU's plugin panel,
      // and reject any ancestor containing a second SKU panel.
      let card = a; let text = panels.get(sku) || ''; let isolated = !!text;
      for (let depth=0; card && depth<18; depth++,card=card.parentElement) {
        if (card === panel || card === document.body) break;
        const t = normalize(card.innerText || '');
        const ids = [...t.matchAll(/SKU[\s:：]*(\d{7,})(?!\d)/gi)].map(m=>m[1]);
        if (ids.some(id=>id!==sku)) break;
        if (ids.includes(sku) && t.length > text.length) { text=t; isolated=true; }
      }
      const img = a.querySelector('img');
      const previous = rows.get(sku) || {};
      const row = {...previous, SKU:sku, '商品链接':url.origin+url.pathname, '店铺链接':location.origin+store+'/',
        '商品标题':a.innerText?.trim() || img?.alt || previous['商品标题'] || '',
        '图片链接':img?.currentSrc || img?.src || previous['图片链接'] || '',
        '采集时间':new Date().toISOString(), '来源页面':location.href.split('#')[0]};
      if (isolated) {
        for (const [name,label] of Object.entries({'重量原文':'重量','尺寸原文':'体积','类目':'类目','库存原文':'库存','品牌':'品牌','卖家':'卖家','上架时间':'上架时间','近30天销量':'近30天销量','近30天销售额':'近30天销售额','跟卖数':'跟卖数','配送方式':'配送','变体数':'变体数','毛利率':'毛利率','退货取消率':'退货取消率','曝光量':'曝光量','产品卡浏览量':'产品卡浏览量','加购率':'加购率','广告费用份额':'广告费用份额'})) {
          const value=field(text,label); if(value) row[name]=value;
        }
        row['Seerfar面板原文']=text;
      }
      row['归属核对'] = verified(row) ? '店铺名称与插件卖家一致' : '待核对';
      row['缺失字段']=['重量原文','尺寸原文','类目'].filter(k=>!row[k]).join('、');
      rows.set(sku,row); found++;
    }
    if (found) pages.add(location.href.split('#')[0]);
    persist(); show('缺少重量/尺寸/类目 '+[...rows.values()].filter(r=>r['缺失字段']).length+' 个；完整性未确认。');
  }
  const button=(label,fn)=>{ const b=document.createElement('button');b.textContent=label;b.style.cssText='margin:5px;padding:6px;cursor:pointer;background:#fff;color:#14213d;border:1px solid #9aa9c3;border-radius:4px;opacity:1';b.onclick=fn;panel.append(b); };
  const download=(name,content,type)=>{const url=URL.createObjectURL(new Blob([content],{type}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),2000);};
  const csvCell=v=>'"'+String(v??'').replace(/^[=+@\-\t\r]/,"'$&").replace(/"/g,'""')+'"';
  button('采集当前已加载商品',capture);
  async function scan() {
    if(running)return;running=true;let idle=0; if (window.scrollY > 0) window.scrollTo(0,0);
    show('正在滚动，等待商品和插件面板加载…');
    while(running && idle<5) {
      capture();
      const boundary = recommendationBoundary();
      if (boundary && boundary.getBoundingClientRect().top <= innerHeight) {
        // Give the last store cards time to finish rendering before leaving.
        show('已到推荐区域，等待最后的插件数据…');
        await new Promise(r=>setTimeout(r,5000));
        if (!running) break;
        capture(); running=false;
        if (auto && findNextPage()) { nextPage(); return; }
        auto=false;persist();
        show('已到推荐区域并停止；无法确认店铺总数或是否存在其他分页，请核对后导出。');
        return;
      }
      const y=window.scrollY;window.scrollBy(0,Math.max(400,innerHeight*0.7));
      await new Promise(r=>setTimeout(r,2500));
      idle=window.scrollY===y?idle+1:0;
    }
    if(!running) return;
    capture();running=false;
    if(auto && findNextPage()) nextPage();
    else {auto=false;persist();show('滚动已停止：未确认推荐区或下一页，完整性未确认，请核对后导出。');}
  }
  button('滚动采集本页',()=>{auto=false;void scan();});
  button('自动翻页采集',()=>{auto=true;persist();void scan();});
  button('停止',()=>{running=false;auto=false;persist();});
  function findNextPage() { return [...document.querySelectorAll('a[href]')].find(a=>a.rel==='next'||/^(下一页|Дальше|Следующая)$/i.test((a.innerText || '').trim())); }
  function nextPage() {
    running=false;capture();
    const next=findNextPage();
    if(!next){auto=false;persist();show('未识别下一页，已停止；请核对是否末页，完整性未确认。');return;}
    const u=new URL(next.href,location.href);if(u.origin!==location.origin||!u.pathname.startsWith(store+'/')){show('下一页链接不属于当前店铺，请人工核对。');return;}
    if(pages.has(u.href.split('#')[0]) || pages.size>=500){auto=false;persist();show('检测到重复页或达到500页上限，已停止，请导出核对。');return;}
    u.hash='ozon-erp-shop';persist();location.href=u.href;
  }
  button('采集并前往下一页',nextPage);
  button('导出 CSV 表格',()=>{capture();const data=[...rows.values()].filter(verified).map(r=>({...r,'归属核对':'店铺名称与插件卖家一致'}));if(!data.length){show('没有核对通过的商品，请等待 Seerfar 面板加载并确认店铺名称。');return;}const headers=[...new Set(data.flatMap(Object.keys))];download('ozon-shop-'+store.split('/').pop()+'.csv','\ufeff'+[headers,...data.map(r=>headers.map(h=>r[h]))].map(r=>r.map(csvCell).join(',')).join('\r\n'),'text/csv;charset=utf-8');});
  button('导出原始 JSON',()=>{capture();download('ozon-shop.json',JSON.stringify({store,completeness:'unverified',pages:[...pages],products:[...rows.values()].filter(verified),unverified:[...rows.values()].filter(r=>!verified(r))},null,2),'application/json');});
  document.body.append(panel);capture();
  if(auto) void scan();
})();
