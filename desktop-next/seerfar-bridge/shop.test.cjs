const {test}=require('node:test');const assert=require('node:assert/strict');const vm=require('node:vm');const fs=require('node:fs');
test('store capture isolates SKU panels and keeps raw fields',()=>{
 const saved={};const buttons=[];
 const body={append(){}};
 function card(sku,weight){const parent={innerText:`SKU: ${sku}\n重量: ${weight} g\n体积: 70×64×25mm\n类目: 配件`,parentElement:body};return {href:`https://www.ozon.ru/product/test-${sku}/`,innerText:'产品 '+sku,parentElement:parent,querySelector(){return null;}};}
 const links=[card('4241394455',33),card('4291018150',7)];
 const context={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem:k=>saved[k],setItem:(k,v)=>saved[k]=v},document:{body,createElement(){return {style:{},dataset:{},append(){},click(){}};},querySelectorAll(selector){return selector==='body *' ? [] : links;}},URL,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),context);
 const data=JSON.parse(Object.values(saved)[0]);assert.equal(data.rows.length,2);assert.equal(data.rows[0]['重量原文'],'33 g');assert.equal(data.rows[1]['重量原文'],'7 g');assert.equal(data.rows[0]['缺失字段'],'');assert.equal(data.pages.length,1);
});

test('sibling Seerfar panels bind by SKU with inline size and stock',()=>{
 const saved={};const body={append(){}};
 const grid={innerText:'SKU: 4449275314\nSKU: 4291018150',parentElement:body};
 const detail={innerText:'SKU: 4449275314\n重量: 20 g 体积: 95×75×19mm\n类目: 门锁舌 库存: 7066',parentElement:grid};
 const seed={textContent:'SKU: 4449275314',innerText:'SKU: 4449275314',parentElement:detail};
 const other={innerText:'SKU: 4291018150\n重量: 7 g\n体积: 73×50×11mm\n类目: 水质检测仪',parentElement:grid};
 const seed2={textContent:'SKU: 4291018150',innerText:'SKU: 4291018150',parentElement:other};
 const links=['4449275314','4291018150'].map(sku=>({href:'https://www.ozon.ru/product/'+sku+'/',innerText:'商品',parentElement:grid,querySelector(){return null;}}));
 const ctx={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem:k=>saved[k],setItem:(k,v)=>saved[k]=v},document:{body,createElement(){return {style:{},dataset:{},append(){}};},querySelectorAll(selector){return selector==='body *'?[seed,seed2]:links;}},URL,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),ctx);
 const rows=JSON.parse(Object.values(saved)[0]).rows;
 assert.equal(rows[0]['重量原文'],'20 g');assert.equal(rows[0]['尺寸原文'],'95×75×19mm');assert.equal(rows[0]['类目'],'门锁舌');assert.equal(rows[0]['库存原文'],'7066');assert.equal(rows[1]['重量原文'],'7 g');
});

test('actual colon-free rendered plugin text is parsed',()=>{
 const saved={};const body={append(){}};const parent={innerText:'SKU\n4449275314\n近30天销量7455\n重量20 g\n体积95×75×19mm\n类目门锁舌\n库存7066',parentElement:body};
 const link={href:'https://www.ozon.ru/product/4449275314/',innerText:'门锁',parentElement:parent,querySelector(){return null;}};
 const ctx={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem:k=>saved[k],setItem:(k,v)=>saved[k]=v},document:{body,createElement(){return {style:{},dataset:{},append(){}};},querySelectorAll(s){return s==='body *'?[]:[link];}},URL,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),ctx);const r=JSON.parse(Object.values(saved)[0]).rows[0];
 assert.equal(r['重量原文'],'20 g');assert.equal(r['尺寸原文'],'95×75×19mm');assert.equal(r['类目'],'门锁舌');assert.equal(r['库存原文'],'7066');assert.equal(r['近30天销量'],'7455');
});

test('CSV excludes old recommendations and mismatched sellers',()=>{
 const saved={};const buttons=[];const body={append(){}};let downloaded;
 const old=[{SKU:'1111111','卖家':'Выгодный Маркет本土'},{SKU:'2222222','卖家':'Другой продавец'},{SKU:'3333333'}];
 class B{constructor(parts){this.text=parts.join('')}};class U extends URL{};U.createObjectURL=b=>{downloaded=b.text;return 'blob:test'};U.revokeObjectURL=()=>{};
 const doc={body,querySelector(s){return s==='h1'?{innerText:'Выгодный Маркет'}:null},querySelectorAll(){return []},createElement(tag){const e={style:{},dataset:{},append(){},click(){}};if(tag==='button')buttons.push(e);return e;}};
 const ctx={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem(){return JSON.stringify({rows:old,pages:[]})},setItem(k,v){saved[k]=v}},document:doc,URL:U,Blob:B,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),ctx);buttons.find(b=>b.textContent==='导出 CSV 表格').onclick();
 assert.match(downloaded,/1111111/);assert.doesNotMatch(downloaded,/2222222|3333333/);assert.equal(JSON.parse(Object.values(saved)[0]).rows.length,3);
});
test('recommendation heading excludes later product links',()=>{
 const saved={};const body={append(){}};const heading={innerText:'为您推荐',getClientRects(){return [1]},compareDocumentPosition(a){return a.recommended?4:2}};
 const links=[{href:'https://www.ozon.ru/product/1111111/',recommended:false,innerText:'店铺商品',parentElement:body,querySelector(){return null}},{href:'https://www.ozon.ru/product/2222222/',recommended:true,innerText:'推荐商品',parentElement:body,querySelector(){return null}}];
 const doc={body,querySelector(){return null},querySelectorAll(s){return s==='h1,h2,h3,h4,[role=heading]'?[heading]:s==='body *'?[]:links},createElement(){return {style:{},dataset:{},append(){}}}};
 const ctx={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem(){return null},setItem(k,v){saved[k]=v}},document:doc,Node:{DOCUMENT_POSITION_FOLLOWING:4},URL,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),ctx);const rows=JSON.parse(Object.values(saved)[0]).rows;assert.equal(rows.length,1);assert.equal(rows[0].SKU,'1111111');
});
test('store title fallback and late heading verify existing captured seller',()=>{
 const saved={};const buttons=[];const body={append(){}};let heading='';let downloaded='';
 class B{constructor(parts){this.text=parts.join('')}};class U extends URL{};U.createObjectURL=b=>{downloaded=b.text;return 'blob:test'};U.revokeObjectURL=()=>{};
 const doc={body,title:'Выгодный Маркет——OZON官方网店，商品目录',querySelector(s){return s==='h1'&&heading?{innerText:heading}:null},querySelectorAll(){return []},createElement(tag){const e={style:{},dataset:{},append(){},click(){}};if(tag==='button')buttons.push(e);return e;}};
 const ctx={location:{hash:'#ozon-erp-shop',pathname:'/seller/ghl0505/',origin:'https://www.ozon.ru',href:'https://www.ozon.ru/seller/ghl0505/'},sessionStorage:{getItem(){return JSON.stringify({rows:[{SKU:'1111111','卖家':'Выгодный Маркет本土'}]})},setItem(k,v){saved[k]=v}},document:doc,URL:U,Blob:B,Map,Set,Date,JSON,console,setTimeout};
 vm.runInNewContext(fs.readFileSync(__dirname+'/shop.js','utf8'),ctx);buttons.find(b=>b.textContent==='导出 CSV 表格').onclick();assert.match(downloaded,/1111111/);
 doc.title='';heading='Выгодный Маркет';downloaded='';buttons.find(b=>b.textContent==='导出 CSV 表格').onclick();assert.match(downloaded,/1111111/);
});
