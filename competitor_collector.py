"""Visible Ozon public-product collector adapted from the legacy RFBS tool.

The process prints exactly one JSON object to stdout. It does not import browser
cookies, bypass CAPTCHA, or hide browser automation. Sales is intentionally null
because the public product page does not expose a verifiable cumulative value.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import time
from pathlib import Path


def article(value: str) -> str:
    matches = re.findall(r"(?<!\d)(\d{6,})(?!\d)", value or "")
    return matches[-1] if matches else ""


def collect(url: str, expected: str, profile: Path, timeout: int) -> dict:
    from playwright.sync_api import sync_playwright

    profile.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as playwright:
        context = None
        last_error = None
        for channel in ("msedge", "chrome"):
            try:
                context = playwright.chromium.launch_persistent_context(
                    str(profile), channel=channel, headless=False, locale="ru-RU",
                    viewport={"width": 1360, "height": 900},
                )
                break
            except Exception as error:
                last_error = error
        if context is None:
            raise RuntimeError(f"cannot launch Edge/Chrome: {last_error}")
        try:
            page = context.pages[0] if context.pages else context.new_page()
            try:
                page.goto(url, wait_until="domcontentloaded", timeout=min(timeout * 1000, 120000))
            except Exception:
                pass
            deadline = time.monotonic() + timeout
            last_signature = ""
            stable = 0
            while time.monotonic() < deadline:
                if page.is_closed():
                    raise RuntimeError("browser window was closed")
                try:
                    data = page.evaluate(r"""
                () => {
                  const visible = el => { if (!el) return false; const r=el.getBoundingClientRect(),s=getComputedStyle(el); return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'; };
                  const roots=[...document.querySelectorAll('[data-widget*="Price"],[data-widget*="price"],[class*="price"],[class*="Price"]')].filter(visible);
                  const text=(roots.map(x=>x.innerText||'').join('\n')||document.body?.innerText||'');
                  const match=text.match(/(?:^|\s)(\d[\d\s\u00a0\u202f]{0,12})\s*₽/);
                  const imgs=[...document.querySelectorAll('[data-widget*="Gallery"] img,[data-widget*="gallery"] img,img')].filter(visible);
                  const main=imgs.find(x=>{const r=x.getBoundingClientRect(),u=x.currentSrc||x.src||'';return /^https?:/i.test(u)&&/ozone\.ru/i.test(u)&&r.width>=250&&r.height>=250})||imgs.find(x=>/^https?:/i.test(x.currentSrc||x.src||'')&&/ozone\.ru/i.test(x.currentSrc||x.src||''));
                  return {url:location.href,title:document.querySelector('h1')?.innerText?.trim()||document.title||'',pageTitle:document.title||'',price:match?match[1].replace(/[\s\u00a0\u202f]/g,''):'',image:main?(main.currentSrc||main.src||''):''};
                }
                    """)
                except Exception:
                    if page.is_closed():
                        raise
                    page.wait_for_timeout(1000)
                    continue
                final_url = str(data.get("url") or "")
                observed = article(final_url)
                if expected and observed and observed != expected:
                    raise RuntimeError(f"product identity mismatch: expected {expected}, got {observed}")
                title = str(data.get("title") or "").strip()
                page_title = str(data.get("pageTitle") or "").casefold()
                price = str(data.get("price") or "").strip()
                image = str(data.get("image") or "").strip()
                blocked = any(x in page_title for x in ("похоже", "доступ ограничен", "captcha", "нет соединения"))
                if title and price and image and not blocked:
                    signature = f"{title}|{price}|{image}"
                    stable = stable + 1 if signature == last_signature else 0
                    last_signature = signature
                    if stable >= 1:
                        return {"ok": True, "url": final_url, "product_code": observed or expected, "name": title, "price": float(price), "image": image, "sales": None, "source": "legacy_python_playwright"}
                page.wait_for_timeout(1000)
            raise RuntimeError("timed out waiting for a complete public product page")
        finally:
            context.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--expected", required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()
    try:
        result = collect(args.url, args.expected, Path(args.profile), args.timeout)
    except Exception as error:
        result = {"ok": False, "error": f"{type(error).__name__}: {error}"}
    sys.stdout.write(json.dumps(result, ensure_ascii=False))
    return 0 if result.get("ok") else 2


if __name__ == "__main__":
    raise SystemExit(main())
