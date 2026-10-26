import sqlite3,json,datetime,collections
from pathlib import Path
v=json.loads(Path('outputs/ozon-cancel-diagnosis/live.json').read_text(encoding='utf-8'));assert set(v)=={'FBS','FBO'}
c=sqlite3.connect('data-next/shops/shop_next_9ce5632eec80.db',timeout=30)
backup={t:c.execute('select * from '+t+' where day between ? and ?',('2026-10-17','2026-10-24')).fetchall() for t in ['posting_routes','cancellation_events']}
Path('outputs/ozon-cancel-diagnosis/before-backfill.json').write_text(json.dumps(backup,ensure_ascii=False),encoding='utf-8')
now=datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
with c:
 c.execute("CREATE TABLE IF NOT EXISTS posting_sync_coverage(day TEXT NOT NULL,scheme TEXT NOT NULL,status TEXT NOT NULL,synced_at TEXT NOT NULL,error TEXT NOT NULL DEFAULT '',PRIMARY KEY(day,scheme))")
 for scheme,data in v.items():
  assert data['has_next']==False
  count=0
  for r in data['postings']:
   day=(r.get('created_at') or r.get('in_process_at'))[:10] if scheme=='FBO' else (r.get('in_process_at') or r.get('created_at'))[:10]
   a=r.get('analytics_data') or {};origin=a.get('warehouse_name') or (r.get('delivery_method') or {}).get('warehouse') or '';dest=a.get('region') or a.get('city') or ''
   for i,p in enumerate(r['products']):
    sku=str(p['sku']);rid=f"seller_{scheme.lower()}:{r['posting_number']}:{sku}:{i}";money=p.get('price',0);price=float(money.get('amount',0)) if isinstance(money,dict) and money.get('currency','RUB')=='RUB' else float(money) if not isinstance(money,dict) else 0
    c.execute("INSERT INTO posting_routes(event_id,day,sku,offer_id,product_name,quantity,scheme,status,posting_number,origin,destination,order_price,source,updated_at)VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,CURRENT_TIMESTAMP) ON CONFLICT(event_id) DO UPDATE SET day=excluded.day,offer_id=excluded.offer_id,product_name=excluded.product_name,quantity=excluded.quantity,status=excluded.status,origin=excluded.origin,destination=excluded.destination,order_price=excluded.order_price,updated_at=CURRENT_TIMESTAMP",(rid,day,sku,p.get('offer_id',''),p.get('name',''),p['quantity'],scheme,r['status'],r['posting_number'],origin,dest,price,'seller_'+scheme.lower()));count+=1
  for day in [f'2026-10-{d}' for d in range(17,25)]:c.execute("INSERT INTO posting_sync_coverage(day,scheme,status,synced_at,error) VALUES(?,?,'success',?,'') ON CONFLICT(day,scheme) DO UPDATE SET status='success',synced_at=excluded.synced_at,error=''",(day,scheme,now))
  c.execute("INSERT INTO sync_logs(started_at,finished_at,source,status,rows_count,message) VALUES(CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,?,'success',?,?)",('Seller Orders '+scheme,count,'新版游标接口回补2026-10-17至2026-10-24，已读取全部分页并核对取消状态'))
 c.execute("DELETE FROM cancellation_events WHERE day BETWEEN '2026-10-17' AND '2026-10-24'")
 c.execute("INSERT INTO cancellation_events(event_id,day,sku,offer_id,product_name,quantity,scheme,source,updated_at) SELECT 'posting:'||event_id,day,sku,offer_id,product_name,quantity,scheme,'posting_status',CURRENT_TIMESTAMP FROM posting_routes WHERE day BETWEEN '2026-10-17' AND '2026-10-24' AND lower(status) LIKE '%cancel%'")
summary=c.execute("SELECT day,SUM(quantity) FROM cancellation_events WHERE day BETWEEN '2026-10-17' AND '2026-10-24' GROUP BY day ORDER BY day").fetchall();print(json.dumps({'cancelled_units_by_order_day':summary,'total':sum(q for d,q in summary)}))
Path('outputs/ozon-cancel-diagnosis/summary.json').write_text(json.dumps(summary),encoding='utf-8')
