import { campaignLabel, canQueryCampaign, type WbAdCampaign } from "./wbAdCampaign";
import { WbAdProductAttribution } from "./WbAdProductAttribution";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Daily = { query: string; day: string; clicks: number; orders: number; carts: number; spend: number; views: number | null };
type Counts = { clicks: number; orders: number; carts: number; spend: number; views: number | null };
const weights = ["成本", "下单率", "加购率", "相关性", "成交规模"];
const defaults = { cost: 200, minClicks: 30, minOrders: 3, scaleOrders: 5, stop: 1.5, high: 80, middle: 60, weights: [45, 25, 10, 10, 10] };
const fields = [
  ["cost", "目标 CPA"], ["minClicks", "观察点击门槛"], ["minOrders", "观察订单门槛"],
  ["scaleOrders", "成交满分与放量单量"], ["stop", "零单止损倍数"], ["high", "放量分数线"], ["middle", "优化分数线"],
] as const;
const empty = (): Counts => ({ clicks: 0, orders: 0, carts: 0, spend: 0, views: 0 });
const add = (total: Counts, row: Counts) => {
  total.clicks += row.clicks;
  total.orders += row.orders;
  total.carts += row.carts;
  total.spend += row.spend;
  total.views = total.views === null || row.views === null ? null : total.views + row.views;
};
const before = (end: string, days: number) =>
  new Date(Date.parse(end) - days * 86400000).toISOString().slice(0, 10);
const fmt = (n: number | null, digits = 2) => n === null ? "—" : n.toFixed(digits);
const pct = (n: number | null) => n === null ? "—" : fmt(100 * n) + "%";
const ratio = (n: number, d: number) => d > 0 ? n / d : null;

function load<T>(key: string, fallback: T): T {
  try {
    const saved = localStorage.getItem(key);
    return saved ? JSON.parse(saved) as T : fallback;
  } catch {
    return fallback;
  }
}

export function WbKeywordTemplate({ shopId, campaigns, defaultEnd }: { shopId: string; campaigns: WbAdCampaign[]; defaultEnd: string }) {
  const key = "wb-keyword-template:" + shopId;
  const [template, setTemplate] = useState(() => load(key, defaults));
  const [selectedCampaign, setCampaign] = useState("");
  const active = campaigns.find(item => String(item.id) === selectedCampaign && canQueryCampaign(item));
  const campaign = active ? String(active.id) : "";
  const [product, setProduct] = useState("");
  const [end, setEnd] = useState(defaultEnd);
  const [result, setResult] = useState<{ campaign: string; product: string; end: string; rows: Daily[] } | null>(null);
  const [relevance, setRelevance] = useState(() => load<Record<string, number>>(key + ":relevance", {}));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState("");
  const sum = template.weights.reduce((a, b) => a + b, 0);
  const valid = sum === 100 && template.weights.every(n => n >= 0) &&
    fields.every(([field]) => Number.isFinite(template[field]) && template[field] > 0) &&
    [template.minClicks, template.minOrders, template.scaleOrders].every(Number.isInteger) &&
    template.stop >= 1 && template.high <= 100 && template.middle < template.high;

  async function sync() {
    setBusy(true);
    setError("");
    try {
      const rows = await invoke<Daily[]>("wb_operations", {
        action: "ad_keywords",
        payload: { shopId, campaignId: Number(campaign), nmId: Number(product), from: before(end, 29), to: end },
      });
      setResult({ campaign, product, end, rows });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  function save() {
    try {
      localStorage.setItem(key, JSON.stringify(template));
      localStorage.setItem(key + ":relevance", JSON.stringify(relevance));
      setSaved("模板和相关性标记已保存到当前店铺。");
    } catch {
      setSaved("保存失败，本地存储不可用。");
    }
  }

  const baseline = empty();
  const grouped = new Map<string, Counts & { recent: Counts }>();
  if (result) {
    const mainStart = before(result.end, 13);
    const recentStart = before(result.end, 6);
    for (const row of result.rows) {
      add(baseline, row);
      if (row.day < mainStart) continue;
      const total = grouped.get(row.query) ?? { ...empty(), recent: empty() };
      add(total, row);
      if (row.day >= recentStart) add(total.recent, row);
      grouped.set(row.query, total);
    }
  }
  const against = (value: number | null, reference: number | null) =>
    value !== null && reference !== null && reference > 0 ? Math.min(1, value / reference) : null;
  const rows = [...grouped].map(([query, total]) => {
    const id = JSON.stringify([result?.product, query]);
    const related = relevance[id];
    const cpa = ratio(total.spend, total.orders);
    const conversion = ratio(total.orders, total.clicks);
    const cartRate = ratio(total.carts, total.clicks);
    const factors = [cpa === null ? 0 : cpa === 0 ? 1 : Math.min(1, template.cost / cpa),
      against(conversion, ratio(baseline.orders, baseline.clicks)),
      against(cartRate, ratio(baseline.carts, baseline.clicks)), related ?? null,
      Math.min(1, total.orders / template.scaleOrders)];
    const used = factors.reduce<number>((weight, value, i) => weight + (value === null ? 0 : template.weights[i]), 0);
    const score = valid && used > 0 ? Math.round(factors.reduce(
      (points: number, value, i) => points + (value ?? 0) * template.weights[i], 0) * 100 / used) : null;
    let verdict = "待配置";
    if (score !== null) {
      if (related === 0 || (total.orders === 0 && total.spend >= template.cost * template.stop)) verdict = "止损复核";
      else if (total.clicks < template.minClicks && total.orders < template.minOrders) verdict = "样本不足";
      else if (related === undefined) verdict = "待标相关性";
      else if (score >= template.high && total.orders >= template.scaleOrders && cpa !== null && cpa <= template.cost) verdict = "放量候选";
      else verdict = score >= template.middle ? "继续优化" : "低效复核";
    }
    return { id, query, total, cpa, conversion, cartRate, score, verdict };
  }).sort((a, b) => (b.score ?? -1) - (a.score ?? -1) || b.total.orders - a.total.orders);
  const validId = (id: string) => /^\d+$/.test(id) && Number.isSafeInteger(Number(id)) && Number(id) > 0;

  return <section className="wbo-panel wb-keyword-panel" aria-labelledby="wb-keyword-title">
    <div className="wbo-heading"><h2 id="wb-keyword-title">搜索词效果评估模板</h2></div>
    <form className="wbo-toolbar" onSubmit={event => { event.preventDefault(); void sync(); }}>
      <label>广告活动<select value={campaign} onChange={event => { setCampaign(event.target.value); setProduct(""); }} required>
        <option value="">选择广告活动</option>
        {campaigns.map(item => <option key={item.id} value={item.id} disabled={!canQueryCampaign(item)}>{campaignLabel(item)}</option>)}
      </select></label>
      <label>WB 商品编号{active?.products.length ? <select value={product} onChange={event => setProduct(event.target.value)} required>
        <option value="">选择投放商品</option>
        {active.products.map(id => <option key={id} value={id}>{id}</option>)}
      </select> : <input value={product} onChange={event => setProduct(event.target.value)} required inputMode="numeric" />}</label>
      <label>统计截止日<input type="date" value={end} max={defaultEnd} onChange={event => setEnd(event.target.value)} required /></label>
      <button className="primary" disabled={busy || !validId(campaign) || !validId(product) || !end || end > defaultEnd}>
        {busy ? "正在获取…" : "获取搜索词组"}
      </button>
    </form>
    {error && <p className="wbo-error" role="alert">{error}</p>}
    <fieldset className="wbo-stock-fields">
      <legend>评估参数</legend>
      <div className="wb-keyword-fields">
        {fields.map(([field, label]) => <label key={field}>{label}
          <input type="number" min="0" step="any" value={template[field]} onChange={event => {
            setTemplate({ ...template, [field]: Number(event.target.value) }); setSaved("");
          }} />
        </label>)}
        {weights.map((name, index) => <label key={name}>{name}权重（%）
          <input type="number" min="0" max="100" value={template.weights[index]} onChange={event => {
            setTemplate({ ...template, weights: template.weights.map((n, i) => i === index ? Number(event.target.value) : n) }); setSaved("");
          }} />
        </label>)}
      </div>
      <div className="wbo-actions">
        <button type="button" disabled={!valid} onClick={save}>保存模板</button>
        <button type="button" onClick={() => { setTemplate(defaults); setSaved(""); }}>恢复默认参数</button>
      </div>
    </fieldset>
    <p>目标 CPA 与 WB 广告账户币种一致。权重合计 {sum}%。近 14 天评分，近 7 天观察，近 30 天同活动同商品为基准。缺失项按其余权重折算，相关性未标记时暂不建议放量。</p>
    {!valid && <p className="wbo-error">请将权重合计设为 100%，确保参数为正数、点击和订单门槛为整数，且放量线高于优化线。</p>}
    {saved && <p role="status">{saved}</p>}
    <WbAdProductAttribution shopId={shopId} campaign={campaign} end={end}
      target={template.cost} scale={template.scaleOrders} />
    {result && <>
      <p>活动 {result.campaign} · 商品 {result.product} · 统计至 {result.end} · 近 14 天共 {rows.length} 个搜索词组</p>
      {!rows.length && <p>WB 未返回本期搜索词组明细。</p>}
      {!!rows.length && <div className="wbo-table-wrap"><table>
        <thead><tr>{["搜索词组", "曝光", "点击 / CTR", "归因订单", "花费 / CPA", "下单率 / 加购率", "近 7 天订单 / CPA", "相关性", "得分", "评估"].map(label =>
          <th key={label}>{label}</th>)}</tr></thead>
        <tbody>{rows.map(({ id, query, total, cpa, conversion, cartRate, score, verdict }) => <tr key={id}>
          <td>{query}</td><td>{total.views ?? "—"}</td><td>{total.clicks} / {pct(total.views === null ? null : ratio(total.clicks, total.views))}</td>
          <td>{total.orders}</td><td>{fmt(total.spend)} / {fmt(cpa)}</td><td>{pct(conversion)} / {pct(cartRate)}</td>
          <td>{total.recent.orders} / {fmt(ratio(total.recent.spend, total.recent.orders))}</td>
          <td><select aria-label={query + " 相关性"} value={relevance[id] ?? ""} onChange={event => {
            const next = { ...relevance };
            if (event.target.value === "") delete next[id];
            else next[id] = Number(event.target.value);
            setRelevance(next); setSaved("");
          }}>
            <option value="">待标记</option><option value="1">匹配</option><option value="0.5">部分匹配</option><option value="0">不匹配</option>
          </select></td><td>{score ?? "—"}</td><td>{verdict}</td>
        </tr>)}</tbody>
      </table></div>}
    </>}
  </section>;
}
