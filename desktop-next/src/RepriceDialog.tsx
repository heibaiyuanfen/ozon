import { useEffect, useMemo, useRef, useState } from "react";
import { listShops, priceRepriceSuggestions, priceRepriceValidate, refreshPriceIntelligence, refreshProductPrice, resolveRepriceProductIds, updateProductPrice } from "./bridge";
import type { RepriceSuggestion } from "./types";

type Plan = { kind: "base" | "blocked"; price: number; steps: string[]; reason: string };
const money = (value: number) => `¥${value.toFixed(2)}`;


export function RepriceDialog({ skus, warningMargin, mode, targetMargin = 15, shopId, shopName, onClose, onChanged }: { skus: string[]; warningMargin: number; mode: "manual" | "warning" | "target"; targetMargin?: number; shopId: string; shopName: string; onClose: () => void; onChanged: () => void }) {
  const [suggestions, setSuggestions] = useState<RepriceSuggestion[]>([]);
  const [targets, setTargets] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(0);
  const [notice, setNotice] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [progress, setProgress] = useState<{ done: number; total: number; current: string } | null>(null);
  const stopRequested = useRef(false);
  const mounted = useRef(true);
  const title = mode === "target" ? `按目标利润率 ${targetMargin}% 调价` : mode === "manual" ? "商品手动调价" : "预警商品改价试算";
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; stopRequested.current = true; }; }, []);
  const assertShop = async () => {
    if (!mounted.current) throw new Error("窗口已关闭或已切换店铺，停止后续操作");
    const shops = await listShops();
    if (!shopId || !shops.some((shop) => shop.id === shopId && shop.active && shop.kind === "cross_border")) throw new Error("当前店铺与试算店铺不一致，已停止");
  };

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        await assertShop();
        const items = await priceRepriceSuggestions(skus, mode === "target" ? targetMargin : undefined);
        const missing = items.filter((item) => !/^\d+$/.test(item.productId) || Number(item.productId) <= 0);
        if (missing.length) {
          try {
            const resolved = await resolveRepriceProductIds(missing.map((item) => item.sku));
            for (const item of items) if (resolved[item.sku]) item.productId = resolved[item.sku];
          } catch (error) { if (alive) setNotice(`商品 ID 补查失败：${String(error)}。缺少 ID 的商品仍禁止自动改价。`); }
        }
        if (!alive) return;
        setSuggestions(items);
        setTargets(Object.fromEntries(items.map((item) => [item.sku, (mode === "manual" ? item.currentPriceCny : item.suggestedPriceCny)?.toFixed(2) ?? ""])));
      } catch (error) { if (alive) setNotice(`商品试算失败：${String(error)}`); }
      finally { if (alive) setLoading(false); }
    })();
    return () => { alive = false; };
  }, [skus.join("|"), mode, targetMargin, shopId]);

  const planFor = (item: RepriceSuggestion, target: number): Plan => {
    if (mode === "target" && item.suggestedPriceCny == null) return { kind: "blocked", price: target, steps: [], reason: item.reason };
    if (mode === "target" && item.currentPriceCny != null && Math.abs(target - item.currentPriceCny) < 0.005) return { kind: "blocked", price: target, steps: [], reason: "已在目标利润率上方 0.1 个百分点内，无需改价" };
    if (!item.productId || !/^\d+$/.test(item.productId) || Number(item.productId) <= 0) return { kind: "blocked", price: target, steps: [], reason: "没有可核对的 Ozon 商品 ID" };
    if (!Number.isFinite(target) || target <= 0 || Math.abs(target * 100 - Math.round(target * 100)) > 0.000001) return { kind: "blocked", price: target, steps: [], reason: "请填写大于零且最多两位小数的最终定价" };
    return { kind: "base", price: target, steps: [`提交商品最终定价 ${money(target)}`, "回读商品价格确认"], reason: "Ozon 根据最终定价自动联动促销参与状态" };
  };

  const plans = useMemo(() => Object.fromEntries(suggestions.map((item) => [item.sku, planFor(item, Number(targets[item.sku]))])), [suggestions, targets]);

  const runOne = async (item: RepriceSuggestion, desired: number) => {
    await assertShop();
    const plan = planFor(item, desired);
    if (plan.kind === "blocked") throw new Error(plan.reason);
    const current = await refreshProductPrice(item.sku);
    if (current.currencyCode.toUpperCase() !== "CNY") throw new Error(`商品改价币种是 ${current.currencyCode || "未知"}，无法按人民币自动改价`);
    if (item.productId !== current.productId) throw new Error("商品 ID 已变化，请重新打开试算窗口");
    const baseAlreadyAtTarget = plan.kind === "base" && Math.abs(current.price - plan.price) < 0.005;
    const margin = await priceRepriceValidate(item.sku, plan.price);
    if (mode === "target" && (margin == null || margin + 1e-10 < targetMargin / 100 || margin - targetMargin / 100 > 0.001)) throw new Error("成本或费用参数已变化，目标利润率校验未通过，请重新试算");
    setLog((old) => [...old, `${item.offerId || item.sku}：目标 ${money(plan.price)}，${margin == null ? "成本或重量缺失，无法试算利润率" : `预计利润率 ${(margin * 100).toFixed(2)}%`}，开始执行`]);
      // Submit the final product price; Ozon handles promotion participation.
      const oldPrice = current.oldPrice > plan.price && plan.price > current.oldPrice * 0.1 ? current.oldPrice : 0;
      const minPrice = current.minPrice <= plan.price ? current.minPrice : 0;
      if (baseAlreadyAtTarget) {
        setLog((old) => [...old, `普通售价已是 ${money(plan.price)}，跳过重复提交，继续核对价格`]);
      } else {
        await assertShop();
        await updateProductPrice({ sku: item.sku, price: plan.price, oldPrice, minPrice, currencyCode: "CNY" }, shopId);
        setLog((old) => [...old, `已提交普通售价 ${money(plan.price)}`]);
      }
    await assertShop();
    const verified = await refreshProductPrice(item.sku);
    if (verified.productId !== item.productId || verified.currencyCode.toUpperCase() !== "CNY" || Math.abs(verified.price - plan.price) > 0.005) throw new Error("定价请求已提交，回读价格尚未确认；请稍后刷新核对，勿重复提交");
    setLog((old) => [...old, `${item.offerId || item.sku}：最终定价 ${money(plan.price)} 已回读确认；促销参与状态由 Ozon 联动处理，前台展示可能延迟。`]);
  };

  const run = async (items: RepriceSuggestion[], automatic: boolean) => {
    if (busy) return;
    const actionable = items.filter((item) => {
      const target = automatic ? item.suggestedPriceCny : Number(targets[item.sku]);
      return target != null && (mode === "target" || !automatic || (item.currentPriceCny != null && target <= item.currentPriceCny * 1.2)) && planFor(item, target).kind !== "blocked";
    });
    if (!actionable.length) { setNotice("没有可执行的商品；请查看每行原因"); return; }
    const question = mode === "target" ? `确认调整店铺「${shopName}」的 ${actionable.length} 个商品至目标预计利润率 ${targetMargin}%？包含涨价和降价；不限制 20% 涨幅，请核对预览。Ozon 会根据最终定价联动促销参与状态。` : automatic ? `按试算价自动处理 ${actionable.length} 个商品？自动涨幅限制为当前售价的 20%；已完成的步骤无法自动回滚。` : `确认将 ${actionable[0].offerId || actionable[0].sku} 调至 ${money(Number(targets[actionable[0].sku]))}？${planFor(actionable[0], Number(targets[actionable[0].sku])).steps.join(" → ")}。已提交的步骤无法自动回滚。`;
    if (!window.confirm(question)) return;
    stopRequested.current = false;
    setBusy(true); setNotice(""); setLog([]); setProgress({ done: 0, total: actionable.length, current: "准备核对商品价格" });
    try {
      await assertShop();
      let done = 0;
      for (let offset = 0; offset < actionable.length; offset += 5) {
        if (stopRequested.current) break;
        const group = actionable.slice(offset, offset + 5);
        const completed: string[] = [];
        for (const item of group) {
          if (stopRequested.current) break;
          setProgress({ done, total: actionable.length, current: `正在处理 ${item.offerId || item.sku}` });
          const desired = automatic ? item.suggestedPriceCny! : Number(targets[item.sku]);
          try { await runOne(item, desired); completed.push(item.sku); }
          catch (error) { completed.push(item.sku); setLog((old) => [...old, `${item.offerId || item.sku}：停止，${String(error)}。请检查已完成步骤。`]); if (!automatic) stopRequested.current = true; }
          done += 1;
          setProgress({ done, total: actionable.length, current: `${item.offerId || item.sku} 已处理` });
          await new Promise((resolve) => window.setTimeout(resolve, 0));
        }
        if (completed.length) {
          setProgress({ done, total: actionable.length, current: `正在刷新 ${completed.length} 件商品的缓存` });
          try {
            await assertShop(); const refreshed = await refreshPriceIntelligence(completed); onChanged();
            if (refreshed.errors.length) setLog((old) => [...old, `缓存刷新提示：${refreshed.errors.join("；")}`]);
            if (mode === "target") for (const sku of completed) {
              const row = refreshed.data.rows.find((r) => r.sku === sku);
              const wanted = actionable.find((item) => item.sku === sku)?.suggestedPriceCny;
              const verified = row?.finalPricingCny != null && wanted != null && Math.abs(row.finalPricingCny-wanted) < 0.005 && row.profitMargin != null && row.profitMargin + 1e-10 >= targetMargin/100 && row.profitMargin-targetMargin/100 <= 0.001;
              setLog((old) => [...old, `${row?.offerId || sku}：${verified ? "回读价格及预计利润率已达目标" : "尚未确认达到目标；请刷新后核对平台价格、活动状态与费用参数"}`]);
            }
          }
          catch (error) { setLog((old) => [...old, `改价步骤已提交，但缓存刷新失败：${String(error)}`]); }
        }
      }
      if (stopRequested.current) setLog((old) => [...old, "已停止后续商品；当前件及此前已提交的步骤不会回滚。​"]);
    } catch (error) { setNotice(String(error)); } finally { setBusy(false); setProgress(null); }
  };

  return <div className="pi-modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}>
    <section className="pi-modal" role="dialog" aria-modal="true" aria-label={title}>
      <header><div><h2>{title}</h2><p>{shopName} · {skus.length} 个商品。核对商品最终定价后再提交。</p></div><button disabled={busy} onClick={onClose}>关闭</button></header>
      <div className="pi-modal-note">仅跨境店使用人民币利润模型；预警线为 {warningMargin}%，手动调价可自行决定目标利润率。直接修改商品最终定价，由 Ozon 联动促销参与状态；币种无法核对时不执行改价。提交后回读商品价格，未确认时请稍后刷新核对。</div>
      {mode === "target" && <div className="pi-modal-note">目标与预警线独立；预计利润率允许在目标上方 0.1 个百分点内，受价格精度或阶梯费用限制的商品跳过。确认后执行本次批量调价，不会持续后台改价。实际结算利润可能不同。</div>}
      {notice && <div className="sync-message">{notice}</div>}
      {!loading && <div className="pi-preview-summary">预览 {suggestions.length} 个 · 可执行 {suggestions.filter((item) => plans[item.sku]?.kind !== "blocked").length} 个 · 跳过 {suggestions.filter((item) => plans[item.sku]?.kind === "blocked").length} 个 <button disabled={busy || page === 0} onClick={() => setPage((p) => p-1)}>上一页</button> {page+1}/{Math.max(1, Math.ceil(suggestions.length/50))} <button disabled={busy || (page+1)*50 >= suggestions.length} onClick={() => setPage((p) => p+1)}>下一页</button></div>}
      {loading ? <p>正在读取商品价格及利润参数…</p> : <div className="pi-reprice-list">{suggestions.slice(page*50,(page+1)*50).map((item) => {
        const plan = plans[item.sku];
        return <article key={item.sku}>
          <div className="pi-reprice-title"><strong>{item.offerId || item.sku}</strong><span>当前 {item.currentPriceCny == null ? "—" : money(item.currentPriceCny)} / {item.currentMargin == null ? "—" : `${(item.currentMargin * 100).toFixed(2)}%`} → 建议 {item.suggestedPriceCny == null ? "无法试算" : money(item.suggestedPriceCny)} / {item.projectedMargin == null ? "—" : `${(item.projectedMargin * 100).toFixed(2)}%`}{mode === "target" && item.currentPriceCny != null && item.suggestedPriceCny != null && ` · ${item.suggestedPriceCny > item.currentPriceCny ? "涨价" : item.suggestedPriceCny < item.currentPriceCny ? "降价" : "不变"} ${money(Math.abs(item.suggestedPriceCny-item.currentPriceCny))}`}</span></div>
          <div className="pi-reprice-target"><label>最终定价（CNY） <input type="number" min="0.01" step="0.01" disabled={busy || mode === "target"} value={targets[item.sku] ?? ""} onChange={(e) => setTargets((old) => ({ ...old, [item.sku]: e.target.value }))}/></label><button disabled={busy || loading || !plan || plan.kind === "blocked"} onClick={() => run([item], mode === "target")}>按此价格提交</button></div>
          <small>{plan?.kind === "blocked" ? `暂不能自动处理：${plan.reason}` : `${plan?.reason}；${plan?.steps.join(" → ") || item.reason}`}</small>
        </article>;
      })}</div>}
      {log.length > 0 && <div className="pi-reprice-log" aria-live="polite">{log.map((line, index) => <p key={index}>{line}</p>)}</div>}
      {progress && <div className="pi-progress" aria-live="polite"><div><strong>{progress.current}</strong><span>{progress.done}/{progress.total}</span><button disabled={stopRequested.current} onClick={() => { stopRequested.current = true; }}>完成当前件后停止</button></div><progress max={progress.total} value={progress.done}/></div>}
      <footer><span>{mode === "target" ? "按预览价双向调整；不可执行及已达标商品跳过。" : "不会自动抬高目标价；预警自动模式跳过涨幅超过 20% 的商品。"}</span>{mode !== "manual" && <button disabled={busy || loading || !suggestions.some((item) => plans[item.sku]?.kind !== "blocked")} onClick={() => run(suggestions, true)}>{busy ? "正在执行…" : mode === "target" ? "确认批量调整可执行商品" : "按建议价自动处理可执行商品"}</button>}</footer>
    </section>
  </div>;
}
