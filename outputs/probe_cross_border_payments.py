import json
import sqlite3
import sys
import urllib.request
import urllib.error
from pathlib import Path

root = Path(__file__).resolve().parents[1]
sys.stdout.reconfigure(encoding='utf-8')
sys.path.insert(0, str(root))
from ozon_app.secrets import unprotect

out = root / 'outputs' / 'cross-border-payment-probe'
out.mkdir(exist_ok=True)
registry = json.loads((root / 'data-next/shops.json').read_text(encoding='utf-8'))
for shop in registry['shops']:
    if shop.get('kind') != 'cross_border':
        continue
    db = root / 'data-next' / shop['database_file']
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as c:
        settings = dict(c.execute('SELECT key,value FROM settings WHERE key IN (\'seller_client_id\',\'seller_api_key\')'))
    headers = {'Client-Id': settings.get('seller_client_id', ''), 'Api-Key': unprotect(settings.get('seller_api_key', '')), 'Content-Type': 'application/json'}
    if not headers['Client-Id'] or not headers['Api-Key']:
        print('Missing credentials', shop['name'])
        continue
    payload = {'date': {'from': '2026-09-01T00:00:00.000Z', 'to': '2026-09-30T23:59:59.999Z'}, 'with_details': True, 'page': 1, 'page_size': 100}
    endpoint = sys.argv[1] if len(sys.argv) > 1 else '/v1/finance/cash-flow-statement/list'
    if endpoint == '/v1/finance/balance':
        payload = {'date_from': '2026-09-01', 'date_to': '2026-09-30'}
    if endpoint == '/v1/actions':
        payload = None
    if endpoint == '/v2/actions/products':
        payload = {'action_id': 1977747, 'limit': 1, 'last_id': ''}
    if len(sys.argv) > 2:
        payload = json.loads(sys.argv[2])
    req = urllib.request.Request('https://api-seller.ozon.ru' + endpoint, data=None if payload is None else json.dumps(payload).encode(), headers=headers, method='GET' if payload is None else 'POST')
    try:
        with urllib.request.urlopen(req, timeout=45) as response:
            status, raw = response.status, response.read().decode()
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read().decode()
    result = json.loads(raw)
    (out / (shop['id'] + '-' + endpoint.rsplit('/', 1)[-1] + '-2026-09.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'shop': shop['name'], 'status': status, 'response': result}, ensure_ascii=False))
