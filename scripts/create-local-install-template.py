import sqlite3
from pathlib import Path

root = Path(__file__).resolve().parents[1]
source = root / "data-next" / "shops" / "shop_next_c115c8fc976d.db"
target = root / "desktop-next" / "src-tauri" / "resources" / "data-next-template" / "ozon_next_default.db"
target.unlink(missing_ok=True)
with sqlite3.connect(source) as source_db, sqlite3.connect(target) as target_db:
    source_db.backup(target_db)
    target_db.execute("PRAGMA foreign_keys=OFF")
    tables = [
        row[0]
        for row in target_db.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
        )
    ]
    for table in tables:
        target_db.execute(f'DELETE FROM "{table.replace(chr(34), chr(34) * 2)}"')
    target_db.commit()
    target_db.execute("VACUUM")
print(f"Created empty local install database: {target} ({target.stat().st_size} bytes)")
