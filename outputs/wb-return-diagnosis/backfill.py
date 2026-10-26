import sqlite3,json,datetime
from pathlib import Path
p=Path('outputs/wb-return-diagnosis/live.json');v=json.loads(p.read_text(encoding='utf-8'));shop=v['shopId'];data=v['resources'];assert set(data)=={'item_returns','sales_returns'}
c=sqlite3.connect('data-next/wb-v2/shop_api_center.db',timeout=15)
assert c.execute("select seller_type from wb_shops where id=?",(shop,)).fetchone()[0]=='cross_border'
assert c.execute('select running from wb_ops_sync where shop_id=?',(shop,)).fetchone()[0]==0, 'Shop currently syncing; do not race'
backup={'shopId':shop,'returns':c.execute("select * from wb_ops_rows where shop_id=? and kind='return'",(shop,)).fetchall(),'sync':c.execute('select * from wb_ops_sync where shop_id=?',(shop,)).fetchall()}
Path('outputs/wb-return-diagnosis/before-backfill.json').write_text(json.dumps(backup,ensure_ascii=False),encoding='utf-8')
now=datetime.datetime.now(datetime.timezone.utc).isoformat();counts={}
with c:
 for source,items in [('item_return',data['item_returns']),('sales_return',[r for r in data['sales_returns'] if r.get('saleID','').startswith('R')])]:
  counts[source]=0
  for item in items:
   raw=dict(item);raw['_source']=source
   if source=='item_return':
    assert raw.get('shkId') is not None
    rid=f"item:{raw['shkId']}:{raw.get('orderId','')}"
   else: rid='sale:'+raw['saleID']
   c.execute("insert into wb_ops_rows(shop_id,kind,id,raw,updated_at) values(?,'return',?,?,?) on conflict(shop_id,kind,id) do update set raw=excluded.raw,updated_at=excluded.updated_at",(shop,rid,json.dumps(raw,ensure_ascii=False),now));counts[source]+=1
 msg='官方接口退货回补：2026-07-27 至 2026-10-24。实物退回 '+str(counts['item_return'])+' 条；买家退货 '+str(counts['sales_return'])+' 条。不同来源单独计数；买家退货不等于可取件。'
 c.execute('update wb_ops_sync set message=message || char(10) || ?,updated_at=? where shop_id=?',(msg,now,shop))
print(json.dumps({'backfilled':counts,'return_cache':c.execute("select count(*) from wb_ops_rows where shop_id=? and kind='return'",(shop,)).fetchone()[0]}))
