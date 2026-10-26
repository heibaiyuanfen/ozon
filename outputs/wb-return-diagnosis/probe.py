import sqlite3,base64,ctypes,json,urllib.request,urllib.error
from pathlib import Path
class Blob(ctypes.Structure):
 _fields_=[('cbData',ctypes.c_ulong),('pbData',ctypes.POINTER(ctypes.c_ubyte))]
def unlock(cipher):
 if cipher.startswith('plain:'): return cipher[6:]
 if not cipher.startswith('dpapi:'): return cipher
 data=base64.b64decode(cipher[6:]);buf=(ctypes.c_ubyte*len(data)).from_buffer_copy(data);src=Blob(len(data),buf);out=Blob()
 if not ctypes.windll.crypt32.CryptUnprotectData(ctypes.byref(src),None,None,None,None,0,ctypes.byref(out)): raise RuntimeError('Cannot unlock local credential')
 token=ctypes.string_at(out.pbData,out.cbData).decode();ctypes.windll.kernel32.LocalFree(out.pbData);return token
p=Path('data-next/wb-v2/shop_api_center.db');c=sqlite3.connect('file:'+str(p.resolve())+'?mode=ro',uri=True)
shop,cipher=c.execute("select s.id,a.encrypted_secret from wb_shops s join wb_api_credentials a on a.shop_id=s.id where s.seller_type='cross_border' and a.status='active' order by a.created_at desc limit 1").fetchone();token=unlock(cipher)
bundle={'shopId':shop,'resources':{}}
for name,url,key in [('item_returns','https://seller-analytics-api.wildberries.ru/api/analytics/v1/item-returns?dateFrom=2026-07-27&dateTo=2026-10-24&limit=1000&offset=0','report'),('sales_returns','https://statistics-api.wildberries.ru/api/v1/supplier/sales?dateFrom=2026-07-27&flag=0',None)]:
 try:
  with urllib.request.urlopen(urllib.request.Request(url,headers={'Authorization':token}),timeout=45) as r:
   v=json.load(r) if r.status!=204 else {};items=v if isinstance(v,list) else v.get(key,[])
   bundle['resources'][name]=items
   ret=[x for x in items if str(x.get('saleID','')).startswith('R')] if name=='sales_returns' else items
   print(json.dumps({'source':name,'count':len(items),'returns':len(ret),'keys':list(items[0]) if items else []},ensure_ascii=True))
 except urllib.error.HTTPError as e: print(json.dumps({'source':name,'http':e.code}))
 except Exception as e: print(json.dumps({'source':name,'error':type(e).__name__,'detail':str(e)[:150]}))

Path("outputs/wb-return-diagnosis/live.json").write_text(json.dumps(bundle,ensure_ascii=False),encoding="utf-8")
