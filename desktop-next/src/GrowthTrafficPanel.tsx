import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DateRange } from "./types";

export type TrafficRow = { sku: string; offerId: string; name: string; imageUrl: string; views: number | null; cartAdds: number | null; orderedUnits: number | null; firstDay: string | null; lastDay: string | null; cachedDays: number; sources: string; updatedAt: string | null };
export type TrafficReport = { rows: TrafficRow[]; totalViews: number; firstDay: string | null; lastDay: string | null; updatedAt: string | null };
type TrafficDay = { day: string; views: number; cartAdds: number; orderedUnits: number; source: string };
export const loadProductTraffic = (range: DateRange) => invoke<TrafficReport>("product_traffic", { range });
const count = (n: number | null) => n == null ? "无数据" : n.toLocaleString("zh-CN");
const conversion = (r: TrafficRow) => r.views != null && r.views > 0 && r.orderedUnits != null ? r.orderedUnits / r.views * 100 : null;
function downloadCsv(rows: TrafficRow[], range: DateRange) {
  const cell = (value: unknown) => `"${String(value ?? "").replace(/^[=+@-]/, "'$&").replace(/"/g, '""')}"`;
  const content = [["排名", "SKU", "货号", "商品名称", "浏览次数", "加购次数", "下单件数", "下单件数/浏览%", "缓存首日", "缓存末日", "有记录天数", "来源"], ...rows.map((r, i) => [i + 1, r.sku, r.offerId, r.name, r.views, r.cartAdds, r.orderedUnits, conversion(r)?.toFixed(2), r.firstDay, r.lastDay, r.cachedDays, r.sources])].map(r => r.map(cell).join(",")).join("\r\n");
  const url = URL.createObjectURL(new Blob(["\uFEFF" + content], { type: "text/csv;charset=utf-8" }));
  const a = document.createElement("a"); a.href = url; a.download = `商品流量排行_${range.from}_${range.to}.csv`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
}
export function GrowthTrafficPanel({ data, range, loading }: { data: TrafficReport | null; range: DateRange; loading: boolean }) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("views-desc");
  const [page, setPage] = useState(0);
  const [chosen, setChosen] = useState<TrafficRow | null>(null);
  const [days, setDays] = useState<TrafficDay[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => { setPage(0); setChosen(null); }, [range.from, range.to, data]);
  useEffect(() => {
    let current = true; setDays(null); setError("");
    if (chosen) void invoke<TrafficDay[]>("product_traffic_days", { range, sku: chosen.sku }).then(v => { if (current) setDays(v); }).catch(e => { if (current) setError(String(e)); });
    return () => { current = false; };
  }, [chosen, range.from, range.to]);
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const [field, direction] = sort.split("-");
    const metric = (r: TrafficRow) => field === "carts" ? r.cartAdds : field === "units" ? r.orderedUnits : field === "conversion" ? conversion(r) : r.views;
    return (data?.rows ?? []).filter(r => `${r.sku} ${r.offerId} ${r.name}`.toLowerCase().includes(q)).sort((a, b) => {
      const av = metric(a), bv = metric(b);
      if (av == null || bv == null) return av == null && bv == null ? a.sku.localeCompare(b.sku) : av == null ? 1 : -1;
      return (direction === "asc" ? av - bv : bv - av) || a.sku.localeCompare(b.sku);
    });
  }, [data, query, sort]);
  const pages = Math.max(1, Math.ceil(rows.length / 50));
  const currentPage = Math.min(page, pages - 1);
  return <section className="card growth-traffic" id="growth-product-traffic">
    <div className="growth-traffic-heading"><div><h2>商品流量排行</h2><p>{range.from} 至 {range.to}（含首尾日期） · 全店 {count(data?.totalViews ?? null)} 次浏览</p></div><button disabled={loading || !rows.length} onClick={() => downloadCsv(rows, range)}>导出当前筛选 CSV</button></div>
    <p className="growth-traffic-note">浏览来自 Seller Analytics 的 hits_view，按 SKU 汇总，非去重访客数。无订单商品也计入；无缓存显示“无数据”，不补零。当前缓存有记录日期：{data?.firstDay ? `${data.firstDay} 至 ${data.lastDay}` : "暂无"}。首末日期不代表中间每天数据齐全。</p>
    <div className="growth-traffic-tools"><label>搜索商品<input aria-label="搜索流量商品" placeholder="SKU、货号或商品名称" value={query} onChange={e => { setQuery(e.target.value); setPage(0); }}/></label><label>排序方式<select aria-label="流量排序方式" value={sort} onChange={e => { setSort(e.target.value); setPage(0); }}><option value="views-desc">浏览量：从高到低</option><option value="views-asc">浏览量：从低到高</option><option value="carts-desc">加购次数：从高到低</option><option value="units-desc">下单件数：从高到低</option><option value="conversion-desc">下单件数 / 浏览：从高到低</option></select></label><span>{rows.length} 个商品 · 有浏览记录 {rows.filter(r => r.views != null).length} 个</span></div>
    <div className="growth-traffic-table"><table><thead><tr><th>排名</th><th>商品 / SKU / 货号</th><th>浏览次数</th><th>流量占比</th><th>加购次数</th><th>下单件数</th><th>下单件数 / 浏览</th><th>缓存天数</th><th>每日明细</th></tr></thead><tbody>{rows.slice(currentPage * 50, (currentPage + 1) * 50).map((r, i) => <tr key={r.sku}><td>{currentPage * 50 + i + 1}</td><td><div className="growth-traffic-product">{r.imageUrl && <img loading="lazy" src={r.imageUrl} alt=""/>}<div><b>{r.name || "未命名商品"}</b><small>货号：{r.offerId || "—"}</small><small>SKU：{r.sku || "未分配 SKU"}</small></div></div></td><td><strong>{count(r.views)}</strong></td><td>{r.views != null && data && data.totalViews > 0 ? `${(r.views / data.totalViews * 100).toFixed(1)}%` : "—"}</td><td>{count(r.cartAdds)}</td><td>{count(r.orderedUnits)}</td><td>{conversion(r) == null ? "—" : `${conversion(r)!.toFixed(2)}%`}</td><td title={`${r.firstDay ?? "—"} 至 ${r.lastDay ?? "—"} · 来源 ${r.sources || "无"}`}>{r.cachedDays} 天</td><td><button disabled={r.views == null || loading} onClick={() => setChosen(r)}>查看每日</button></td></tr>)}</tbody></table>{!rows.length && <div className="empty">{loading ? "正在读取商品流量…" : data ? "当前没有匹配的商品。可调整搜索或日期；数据缺失时请到“数据同步”同步 Seller Analytics。" : "尚未加载流量数据。"}</div>}</div>
    <div className="growth-traffic-pagination"><span>第 {currentPage + 1} / {pages} 页 · 每页 50 件</span><button disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>上一页</button><button disabled={currentPage + 1 >= pages} onClick={() => setPage(currentPage + 1)}>下一页</button></div>
    {chosen && <div className="growth-traffic-overlay"><section role="dialog" aria-modal="true" aria-label="商品每日流量"><div className="growth-traffic-heading"><h2>{chosen.offerId || chosen.sku} · 每日流量</h2><button onClick={() => setChosen(null)}>关闭</button></div><p>{chosen.name}</p><p>SKU：{chosen.sku} · {range.from} 至 {range.to} · {count(chosen.views)} 次浏览</p>{error ? <p role="alert">{error}</p> : days == null ? <p>正在加载…</p> : <div className="growth-traffic-table"><table><thead><tr><th>日期</th><th>浏览次数</th><th>加购次数</th><th>下单件数</th><th>来源</th></tr></thead><tbody>{days.map(d => <tr key={d.day}><td>{d.day}</td><td>{d.views}</td><td>{d.cartAdds}</td><td>{d.orderedUnits}</td><td>{d.source}</td></tr>)}</tbody></table></div>}<p>仅展示已有缓存记录的日期，不把缺失日期当作 0。</p></section></div>}
  </section>;
}
