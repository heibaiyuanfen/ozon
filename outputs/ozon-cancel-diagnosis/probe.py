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

p=Path('data-next/shops/shop_next_9ce5632eec80.db');c=sqlite3.connect('file:'+str(p.resolve())+'?mode=ro',uri=True)
client=c.execute("select value from settings where key='seller_client_id'").fetchone()[0];token=unlock(c.execute("select value from settings where key='seller_api_key'").fetchone()[0])

bundle={}
for model,path in [('FBS','/v4/posting/fbs/list'),('FBO','/v3/posting/fbo/list')]:
 cursor='';seen=set();all_rows=[];pages=0
 while True:
  body={'dir':'ASC','filter':{'since':'2026-10-17T00:00:00Z','to':'2026-10-24T23:59:59Z','status':''},'limit':100,'cursor':cursor,'with':{'analytics_data':True,'financial_data':True}}
  req=urllib.request.Request('https://api-seller.ozon.ru'+path,data=json.dumps(body).encode(),headers={'Client-Id':client,'Api-Key':token,'Content-Type':'application/json'},method='POST')
  with urllib.request.urlopen(req,timeout=45) as response: v=json.load(response)
  assert isinstance(v.get('postings'),list);all_rows.extend(v['postings']);pages+=1
  if not v['has_next']: break
  cursor=v['cursor'];assert cursor and cursor not in seen;seen.add(cursor)
  assert len(all_rows)<=50000
 bundle[model]={'postings':all_rows,'has_next':False,'cursor':''}
 print(json.dumps({'model':model,'pages':pages,'records':len(all_rows),'cancelled_units':sum(p['quantity'] for r in all_rows if 'cancel' in r.get('status','') for p in r['products'])}))
Path('outputs/ozon-cancel-diagnosis/live.json').write_text(json.dumps(bundle,ensure_ascii=False),encoding='utf-8')
