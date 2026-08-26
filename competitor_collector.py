"""Ozon collector shared with the legacy RFBS follow-listing flow.

It retains the legacy persistent browser, identity guard, gallery-slot
extraction and hydration wait, then prints one JSON object for Tauri.
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


def high_resolution(value: str) -> str:
    return re.sub(r"/wc\d+/", "/wc2000/", value, flags=re.I)


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
                    args=["--disable-blink-features=AutomationControlled"],
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
                pass  # A verification page can keep loading while the user verifies.
            deadline = time.monotonic() + timeout
            ready_since = None
            last_signature = ""
            stable_rounds = 0
            hydration_started = False
            best: dict = {}
            best_images: list[str] = []
            while time.monotonic() < deadline:
                if page.is_closed():
                    raise RuntimeError("browser window was closed")
                try:
                    data = page.evaluate(r"""
                    () => {
                      let product = {};
                      for (const script of document.querySelectorAll('script[type="application/ld+json"]')) {
                        try {
                          const raw=JSON.parse(script.textContent||'{}'), queue=Array.isArray(raw)?raw:[raw];
                          for (const item of queue) {
                            if (item && String(item['@type']||'').toLowerCase()==='product') product=item;
                            if (item && Array.isArray(item['@graph'])) queue.push(...item['@graph']);
                          }
                        } catch (_) {}
                      }
                      const meta=key=>document.querySelector(`meta[property="${key}"],meta[name="${key}"]`)?.content||'';
                      const source=image=>{
                        const set=String(image.getAttribute('srcset')||'').split(',').map(x=>x.trim().split(/\s+/)[0]).filter(Boolean);
                        return set.at(-1)||image.currentSrc||image.src||image.getAttribute('data-src')||'';
                      };
                      const normalize=raw=>{
                        let value=String(raw||'').trim();
                        if (value.startsWith('//')) value=`https:${value}`;
                        if (!/^https:\/\//i.test(value)||!/ozone\.ru/i.test(value)||/captcha|abt-challenge|\/icons?\//i.test(value)) return '';
                        try { const u=new URL(value); u.pathname=u.pathname.replace(/\/wc\d+\//i,'/wc2000/'); return u.toString(); }
                        catch (_) { return value; }
                      };
                      const elements=new Set();
                      for (const selector of ['[data-widget="webGallery"] img','[data-widget*="Gallery"] img','[data-widget*="gallery"] img'])
                        for (const image of document.querySelectorAll(selector)) elements.add(image);
                      const records=[...elements].map((image,order)=>{const r=image.getBoundingClientRect();return {value:normalize(source(image)),order,x:r.x,y:r.y,w:r.width,h:r.height};}).filter(x=>x.value);
                      const shown=records.filter(x=>x.w>0&&x.h>0), preview=shown.reduce((a,x)=>!a||x.w*x.h>a.w*a.h?x:a,null);
                      const small=shown.filter(x=>Math.max(x.w,x.h)<=180);
                      const rail=preview?small.filter(x=>x.x+x.w<=preview.x+Math.max(24,preview.w*.12)):[];
                      const slots=(rail.length?rail:(small.length?small:records)).sort((a,b)=>Math.abs(a.y-b.y)>2?a.y-b.y:(a.x-b.x||a.order-b.order));
                      let images=slots.map(x=>x.value);
                      if (!images.length) {
                        const raw=product.image||[];
                        images=(Array.isArray(raw)?raw:[raw]).map(normalize).filter(Boolean);
                        const og=normalize(meta('og:image')); if (og) images.push(og);
                      }
                      const heading=document.querySelector('h1')?.innerText?.trim()||'';
                      if (images.length<2) for (const image of document.querySelectorAll('img[src],img[srcset],img[data-src]')) {
                        const r=image.getBoundingClientRect(), alt=String(image.alt||'').trim(), value=normalize(source(image));
                        if (r.width>0&&r.height>0&&value&&alt&&(alt===heading||heading.includes(alt)||alt.includes(heading))) images.push(value);
                      }
                      const unique=[], seen=new Set();
                      for (const value of images) {
                        let key=value;
                        try { const u=new URL(value), m=u.pathname.match(/\/s3\/multimedia[^/]*\/wc\d+\/([^/]+)$/i); key=m?m[1]:u.origin+u.pathname; } catch (_) {}
                        if (!seen.has(key)) { seen.add(key); unique.push(value); }
                      }
                      const priceRoots=[...document.querySelectorAll('[data-widget*="Price"],[data-widget*="price"],[class*="price"],[class*="Price"]')]
                        .filter(node=>{const r=node.getBoundingClientRect();return r.width>0&&r.height>0;});
                      const priceText=priceRoots.map(x=>x.innerText||'').join('\n')||document.body?.innerText||'';
                      const visiblePrice=priceText.match(/(?:^|\s)(\d[\d\s\u00a0\u202f]{0,12})\s*₽/);
                      const structuredPrice=product.offers?.price||product.offers?.lowPrice||'';
                      return {
                        url:location.href,
                        title:String(product.name||meta('og:title')||heading||'').trim(), pageTitle:document.title||'',
                        price:String(visiblePrice?visiblePrice[1].replace(/[\s\u00a0\u202f]/g,''):structuredPrice).replace(',','.'),
                        images:unique.slice(0,20),
                        description:document.querySelector('[data-widget="webDescription"]')?.innerText?.trim()||String(product.description||'').trim(),
                        galleryMode:rail.length?'fixed-left-thumbnail-rail':(small.length?'thumbnail-size-fallback':'gallery-dom-fallback')
                      };
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
                images = list(dict.fromkeys(high_resolution(str(x)) for x in data.get("images", []) if str(x).startswith("https://")))
                blocked = any(x in page_title for x in ("похоже", "доступ ограничен", "captcha", "нет соединения"))
                if title and not blocked:
                    if len(images) >= len(best_images):
                        best, best_images = data, images
                    if not hydration_started:
                        page.evaluate("() => window.scrollTo(0, 0)")
                        hydration_started = True
                        page.wait_for_timeout(1200)
                        continue
                    signature = f"{title}|{price}|{'|'.join(best_images)}"
                    stable_rounds = stable_rounds + 1 if signature == last_signature else 0
                    last_signature = signature
                    ready_since = ready_since or time.monotonic()
                    if price and best_images and stable_rounds >= 2 and time.monotonic() - ready_since >= 3:
                        return {
                            "ok": True, "url": final_url, "product_code": observed or expected,
                            "name": str(best.get("title") or title).strip(), "price": float(price),
                            "image": best_images[0], "images": best_images[:20],
                            "description": str(best.get("description") or "").strip(),
                            "gallery_mode": str(best.get("galleryMode") or ""),
                            "sales": None, "source": "legacy_follow_listing_playwright",
                        }
                page.wait_for_timeout(1000)
            raise RuntimeError("timed out waiting for a stable product page with price and gallery")
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
    # The frozen helper inherits the Windows console code page.  Keep the IPC
    # payload ASCII-only so Rust can always decode it as UTF-8; JSON escapes are
    # restored to the original Unicode text by serde_json.
    sys.stdout.write(json.dumps(result, ensure_ascii=True))
    return 0 if result.get("ok") else 2


if __name__ == "__main__":
    raise SystemExit(main())
