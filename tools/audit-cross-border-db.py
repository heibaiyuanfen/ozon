import json
import sqlite3
import sys

db_path = sys.argv[1]
connection = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
connection.row_factory = sqlite3.Row
products = connection.execute(
    "SELECT sku,offer_id,product_id,name FROM products ORDER BY offer_id,sku"
).fetchall()
costs = connection.execute(
    "SELECT sku,unit_cost_cny,length_cm,width_cm,height_cm,weight_kg,note FROM product_costs ORDER BY sku"
).fetchall()
routes = connection.execute(
    "SELECT COUNT(*) rows,COUNT(DISTINCT posting_number) postings,MIN(day) min_day,MAX(day) max_day FROM posting_routes"
).fetchone()
settings = connection.execute(
    "SELECT key,value FROM settings WHERE key LIKE 'listing_%' OR key LIKE '%rub_per_cny%' ORDER BY key"
).fetchall()
print(json.dumps({
    "products": [dict(row) for row in products],
    "costs": [dict(row) for row in costs],
    "posting_routes": dict(routes),
    "settings": [dict(row) for row in settings],
}, ensure_ascii=False))
