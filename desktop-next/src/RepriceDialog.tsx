import { useEffect, useMemo, useRef, useState } from "react";
import { ozonPromotionProductAction, ozonPromotionProducts, ozonPromotions, priceRepriceSuggestions, priceRepriceValidate, refreshPriceIntelligence, refreshProductPrice, resolveRepriceProductIds, updateProductPrice } from "./bridge";
import type { OzonPromotion, OzonPromotionProduct, RepriceSuggestion } from "./types";

type Membership = { action: OzonPromotion; product: OzonPromotionProduct };
type Check = { memberships: Membership[]; incomplete: boolean };
type Plan = { kind: "promotion" | "base" | "blocked"; price: number; steps: string[]; reason: string };
const money = (value: number) => `¥${value.toFixed(2)}`;
const close = (a: number, b: number) => Math.abs(a - b) <= Math.max(0.05, b * 0.005);

export function RepriceDialog({ skus, warningMargin, mode, onClose, onChanged }: { skus: string[]; warningMargin: number; mode: "manual" | "warning"; onClose: () => void; onChanged: () => void }) {
  const [suggestions, setSuggestions] = useState<RepriceSuggestion[]>([]);
  const [targets, setTargets] = useState<Record<string, string>>({});
  const [checks, setChecks] = useState<Record<string, Check>>({});
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [progress, setProgress] = useState<{ done: number; total: number; current: string } | null>(null);
  const stopRequested = useRef(false);

  const inspect = async (items: RepriceSuggestion[]): Promise<Record<string, Check>> => {
    const catalog = await ozonPromotions(true);
    const active = catalog.promotions.filter((a) => a.isParticipating || a.participatingProductsCount > 0);
    const found: Record<string, Check> = Object.fromEntries(items.map((item) => [item.sku, { memberships: [], incomplete: false }]));
    for (const action of active) {
      try {
        const listing = await ozonPromotionProducts(action.id, "participating");
        const incomplete = listing.total > listing.products.length;
        for (const item of items) {
          if (incomplete) found[item.sku].incomplete = true;
          const product = listing.products.find((p) => String(p.id) === item.productId);
          if (product) found[item.sku].memberships.push({ action, product });
        }
      } catch (error) {
        for (const item of items) found[item.sku].incomplete = true;
        setNotice(`活动 ${action.title || action.id} 查询失败：${String(error)}。自动改价已禁用。`);
      }
    }
    return found;
  };

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const items = await priceRepriceSuggestions(skus);
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
        const result = await inspect(items);
        if (alive) setChecks(result);
      } catch (error) { if (alive) setNotice(`试算或活动查询失败：${String(error)}`); }
      finally { if (alive) setLoading(false); }
    })();
    return () => { alive = false; };
  }, [skus.join("|"), mode]);

  const planFor = (item: RepriceSuggestion, target: number, check?: Check): Plan => {
    if (!item.productId || !/^\d+$/.test(item.productId)) return { kind: "blocked", price: target, steps: [], reason: "没有可核对的 Ozon 商品 ID" };
    if (!check || check.incomplete) return { kind: "blocked", price: target, steps: [], reason: "活动清单未完整读取，无法排除其他促销" };
    if (!Number.isFinite(target) || target <= 0) return { kind: "blocked", price: target, steps: [], reason: "请填写有效的目标售价" };
    if (!check.memberships.length) return { kind: "base", price: target, steps: [`修改普通售价至 ${money(target)}`], reason: "未参加已读取的促销活动" };
    const upper = Math.min(...check.memberships.map(({ product }) => product.maxActionPrice > 0 ? product.maxActionPrice : Infinity));
    const lower = Math.max(...check.memberships.map(({ product }) => product.minActionPrice > 0 ? product.minActionPrice : 0));
    if (target < lower) return { kind: "blocked", price: target, steps: [], reason: `目标价低于活动下限 ${money(lower)}；不会擅自上调目标价` };
    const now = Date.now();
    if (check.memberships.some(({ action }) => action.freezeDate && Date.parse(action.freezeDate) <= now)) return { kind: "blocked", price: target, steps: [], reason: "活动已到冻结期，需在 Ozon 后台核对改价权限" };
    if (!Number.isFinite(upper)) return { kind: "blocked", price: target, steps: [], reason: "Ozon 未返回活动允许最高价，无法安全判断是否需要退出活动" };
    if (target <= upper) {
      return { kind: "promotion", price: target, steps: check.memberships.map(({ action }) => `将「${action.title}」活动价改为 ${money(target)}`), reason: "目标位于活动允许区间" };
    }
    return { kind: "base", price: target, steps: [`先修改普通售价至 ${money(target)}`, ...check.memberships.map(({ action }) => `再退出「${action.title}」`)], reason: `目标超过活动上限 ${money(upper)}` };
  };

  const plans = useMemo(() => Object.fromEntries(suggestions.map((item) => [item.sku, planFor(item, Number(targets[item.sku]), checks[item.sku])])), [suggestions, targets, checks]);

  const runOne = async (item: RepriceSuggestion, desired: number, fresh: Check) => {
    const plan = planFor(item, desired, fresh);
    if (plan.kind === "blocked") throw new Error(plan.reason);
    const current = await refreshProductPrice(item.sku);
    if (current.currencyCode.toUpperCase() !== "CNY") throw new Error(`商品改价币种是 ${current.currencyCode || "未知"}，无法按人民币自动改价`);
    if (item.productId !== current.productId) throw new Error("商品 ID 已变化，请重新打开试算窗口");
    if (fresh.memberships.some(({ product }) => !close(product.price, current.price))) throw new Error("活动价格币种或商品基准价无法与人民币售价核对，请手动处理");
    const margin = await priceRepriceValidate(item.sku, plan.price);
    setLog((old) => [...old, `${item.offerId || item.sku}：目标 ${money(plan.price)}，${margin == null ? "成本或重量缺失，无法试算利润率" : `预计利润率 ${(margin * 100).toFixed(2)}%`}，开始执行`]);
    if (plan.kind === "promotion") {
      for (const { action, product } of fresh.memberships) {
        if (product.maxActionPrice <= 0) throw new Error("Ozon 未返回活动允许最高价，未提交改价");
        if (plan.price < product.actionPrice && product.minActionPrice <= 0) throw new Error("降价时 Ozon 未返回活动最低价，未提交改价");
        const result = await ozonPromotionProductAction({ actionId: action.id, action: "activate", productId: product.id, actionPrice: plan.price, stock: Math.max(product.stock, product.minStock) });
        if (!result.success) throw new Error(`${action.title}：${result.message}`);
        setLog((old) => [...old, `已提交「${action.title}」活动价 ${money(plan.price)}`]);
      }
    } else {
      // Update the base price first. A rejected price must never expose the old base price by exiting a promotion.
      const oldPrice = current.oldPrice > plan.price && plan.price > current.oldPrice * 0.1 ? current.oldPrice : 0;
      const minPrice = current.minPrice <= plan.price ? current.minPrice : 0;
      await updateProductPrice({ sku: item.sku, price: plan.price, oldPrice, minPrice, currencyCode: "CNY" });
      setLog((old) => [...old, `已提交普通售价 ${money(plan.price)}`]);
      for (const { action, product } of fresh.memberships) {
        const result = await ozonPromotionProductAction({ actionId: action.id, action: "deactivate", productId: product.id });
        if (!result.success) throw new Error(`${action.title}：${result.message}；普通售价已提交，活动仍可能生效`);
        setLog((old) => [...old, `已退出「${action.title}」`]);
      }
    }
    setLog((old) => [...old, `${item.offerId || item.sku}：改价步骤已提交，稍后统一刷新缓存。前台补贴价可能延迟，以 Ozon 后台最终显示为准。`]);
  };

  const run = async (items: RepriceSuggestion[], automatic: boolean) => {
    if (busy) return;
    const actionable = items.filter((item) => {
      const target = automatic ? item.suggestedPriceCny : Number(targets[item.sku]);
      return target != null && (!automatic || (item.currentPriceCny != null && target <= item.currentPriceCny * 1.2)) && planFor(item, target, checks[item.sku]).kind !== "blocked";
    });
    if (!actionable.length) { setNotice("没有可执行的商品；请查看每行原因"); return; }
    const question = automatic ? `按试算价自动处理 ${actionable.length} 个商品？自动涨幅限制为当前售价的 20%；已完成的步骤无法自动回滚。` : `确认将 ${actionable[0].offerId || actionable[0].sku} 调至 ${money(Number(targets[actionable[0].sku]))}？${planFor(actionable[0], Number(targets[actionable[0].sku]), checks[actionable[0].sku]).steps.join(" → ")}。已提交的步骤无法自动回滚。`;
    if (!window.confirm(question)) return;
    stopRequested.current = false;
    setBusy(true); setNotice(""); setLog([]); setProgress({ done: 0, total: actionable.length, current: "准备核对活动" });
    try {
      let done = 0;
      for (let offset = 0; offset < actionable.length; offset += 5) {
        if (stopRequested.current) break;
        const group = actionable.slice(offset, offset + 5);
        setProgress({ done, total: actionable.length, current: `重新核对第 ${Math.floor(offset / 5) + 1} 组活动` });
        let checksNow: Record<string, Check>;
        try { checksNow = await inspect(group); }
        catch (error) { setLog((old) => [...old, `活动核对失败，已停止：${String(error)}`]); break; }
        const completed: string[] = [];
        for (const item of group) {
          if (stopRequested.current) break;
          setProgress({ done, total: actionable.length, current: `正在处理 ${item.offerId || item.sku}` });
          const desired = automatic ? item.suggestedPriceCny! : Number(targets[item.sku]);
          try { await runOne(item, desired, checksNow[item.sku]); completed.push(item.sku); }
          catch (error) { setLog((old) => [...old, `${item.offerId || item.sku}：停止，${String(error)}。请检查已完成步骤。`]); if (!automatic) stopRequested.current = true; }
          done += 1;
          setProgress({ done, total: actionable.length, current: `${item.offerId || item.sku} 已处理` });
          await new Promise((resolve) => window.setTimeout(resolve, 0));
        }
        if (completed.length) {
          setProgress({ done, total: actionable.length, current: `正在刷新 ${completed.length} 件商品的缓存` });
          try { const refreshed = await refreshPriceIntelligence(completed); onChanged(); if (refreshed.errors.length) setLog((old) => [...old, `缓存刷新提示：${refreshed.errors.join("；")}`]); }
          catch (error) { setLog((old) => [...old, `改价步骤已提交，但缓存刷新失败：${String(error)}`]); }
        }
      }
      if (stopRequested.current) setLog((old) => [...old, "已停止后续商品；当前件及此前已提交的步骤不会回滚。​"]);
    } finally { setBusy(false); setProgress(null); }
  };

  return <div className="pi-modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}>
    <section className="pi-modal" role="dialog" aria-modal="true" aria-label={mode === "manual" ? "商品手动调价" : "预警商品改价试算"}>
      <header><div><h2>{mode === "manual" ? "商品手动调价" : "预警商品改价试算"}</h2><p>核对目标价与活动方案后再提交；普通售价先更新成功，才会退出促销。</p></div><button disabled={busy} onClick={onClose}>关闭</button></header>
      <div className="pi-modal-note">仅跨境店使用人民币利润模型；预警线为 {warningMargin}%，手动调价可自行决定目标利润率。活动区间、币种或活动清单无法核对时，不执行改价。多个 API 步骤不是事务，部分成功后需人工核查。</div>
      {notice && <div className="sync-message">{notice}</div>}
      {loading ? <p>正在读取活动及商品价格限制…</p> : <div className="pi-reprice-list">{suggestions.map((item) => {
        const plan = plans[item.sku];
        return <article key={item.sku}>
          <div className="pi-reprice-title"><strong>{item.offerId || item.sku}</strong><span>当前 {item.currentPriceCny == null ? "—" : money(item.currentPriceCny)} · 建议 {item.suggestedPriceCny == null ? "无法试算" : money(item.suggestedPriceCny)} · 预计利润率 {item.projectedMargin == null ? "—" : `${(item.projectedMargin * 100).toFixed(2)}%`}</span></div>
          <div className="pi-reprice-target"><label>目标售价（CNY） <input type="number" min="0.01" step="0.01" value={targets[item.sku] ?? ""} onChange={(e) => setTargets((old) => ({ ...old, [item.sku]: e.target.value }))}/></label><button disabled={busy || !plan || plan.kind === "blocked"} onClick={() => run([item], false)}>按此价格提交</button></div>
          <small>{plan?.kind === "blocked" ? `暂不能自动处理：${plan.reason}` : `${plan?.reason}；${plan?.steps.join(" → ") || item.reason}`}</small>
        </article>;
      })}</div>}
      {log.length > 0 && <div className="pi-reprice-log" aria-live="polite">{log.map((line, index) => <p key={index}>{line}</p>)}</div>}
      {progress && <div className="pi-progress" aria-live="polite"><div><strong>{progress.current}</strong><span>{progress.done}/{progress.total}</span><button disabled={stopRequested.current} onClick={() => { stopRequested.current = true; }}>完成当前件后停止</button></div><progress max={progress.total} value={progress.done}/></div>}
      <footer><span>不会自动抬高目标价；自动模式还会跳过涨幅超过 20% 的商品。</span>{mode === "warning" && <button disabled={busy || loading} onClick={() => run(suggestions, true)}>{busy ? "正在执行…" : "按建议价自动处理可执行商品"}</button>}</footer>
    </section>
  </div>;
}
