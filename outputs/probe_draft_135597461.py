import json,sqlite3,sys,urllib.request,urllib.error
from pathlib import Path
sys.stdout.reconfigure(encoding='utf-8')
r=Path.cwd();sys.path.insert(0,str(r))
from ozon_app.secrets import unprotect
reg=json.loads((r/'data-next/shops.json').read_text(encoding='utf-8'));s=next(s for s in reg['shops'] if s['id']=='9ce5632eec80')
with sqlite3.connect((r/'data-next'/s['database_file']).as_uri()+'?mode=ro',uri=True) as c:
 settings=dict(c.execute("select key,value from settings where key in ('seller_client_id','seller_api_key')"))
req=urllib.request.Request('https://api-seller.ozon.ru/v2/draft/create/info',data=json.dumps({'draft_id':135597461}).encode(),headers={'Client-Id':settings['seller_client_id'],'Api-Key':unprotect(settings['seller_api_key']),'Content-Type':'application/json'},method='POST')
try:
 with urllib.request.urlopen(req,timeout=35) as response: status=response.status;raw=response.read().decode()
except urllib.error.HTTPError as e: status=e.code;raw=e.read().decode()
out=r/'outputs/draft-135597461-response.json';out.write_text(raw,encoding='utf-8')
print(json.dumps({'shop':s['name'],'http_status':status,'response':json.loads(raw)},ensure_ascii=False,indent=2))
