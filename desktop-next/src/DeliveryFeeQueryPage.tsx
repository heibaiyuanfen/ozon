import { useMemo, useState } from "react";
import { BadgeRussianRuble, Box, ExternalLink, Search } from "lucide-react";
import { supplyClusterPlans } from "./bridge";
import type { SupplyClusterPlan } from "./types";
import { officialLogisticsTariff, OZON_LOGISTICS_DESTINATIONS, OZON_LOGISTICS_TARIFF_META } from "./ozon-tariffs";
import { ozonClusterZh } from "./ozon-clusters";
import "./delivery-fee-query.css";

type Dimensions = { length: string; width: string; height: string };

export function DeliveryFeeQueryPage() {
  const [query, setQuery] = useState("");
  const [products, setProducts] = useState<SupplyClusterPlan[]>([]);
  const [selected, setSelected] = useState<SupplyClusterPlan | null>(null);
  const [dimensions, setDimensions] = useState<Dimensions>({ length: "", width: "", height: "" });
  const [price, setPrice] = useState("300");
  const [destinationQuery, setDestinationQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const search = async () => {
    if (!query.trim()) return;
    setBusy(true); setError("");
    try {
      const rows = await supplyClusterPlans(30, query.trim());
      const unique = new Map<string, SupplyClusterPlan>();
      rows.forEach((row) => { if (!unique.has(row.sku)) unique.set(row.sku, row); });
      setProducts([...unique.values()]);
      if (!unique.size) setError("没有找到商品，请检查 SKU、货号或商品名称。商品需先同步到当前店铺。");
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };

  const choose = (row: SupplyClusterPlan) => {
    setSelected(row);
    setDimensions({
      length: row.lengthCm ? String(row.lengthCm) : "",
      width: row.widthCm ? String(row.widthCm) : "",
      height: row.heightCm ? String(row.heightCm) : "",
    });
  };
  const volume = useMemo(() => {
    const l = Number(dimensions.length), w = Number(dimensions.width), h = Number(dimensions.height);
    return l > 0 && w > 0 && h > 0 ? l * w * h / 1000 : 0;
  }, [dimensions]);
  const numericPrice = Number(price) || 0;
  const destinations = useMemo(() => OZON_LOGISTICS_DESTINATIONS.filter((name) => {
    const needle = destinationQuery.trim().toLocaleLowerCase();
    return !needle || name.toLocaleLowerCase().includes(needle) || ozonClusterZh(name).includes(needle);
  }).map((name) => ({ name, tariff: officialLogisticsTariff(name, volume) })), [destinationQuery, volume]);

  return <div className="delivery-fee-page">
    <header className="page-header">
      <div><span className="eyebrow">OZON OFFICIAL LOGISTICS TARIFF</span><h1>基础配送费查询</h1><p>从莫斯科发往各配送区域，按商品体积和售价查询官方基础物流费</p></div>
      <a className="delivery-source" href={OZON_LOGISTICS_TARIFF_META.sourceUrl} target="_blank" rel="noreferrer"><ExternalLink size={15}/>查看官方费率表</a>
    </header>

    <section className="card delivery-query-panel">
      <div className="delivery-product-search">
        <label><span>搜索当前店铺商品</span><div><Search size={17}/><input value={query} onChange={(e)=>setQuery(e.target.value)} onKeyDown={(e)=>e.key === "Enter" && search()} placeholder="输入 SKU、货号或商品名称"/><button onClick={search} disabled={busy}>{busy ? "查询中" : "查询商品"}</button></div></label>
        {!!products.length && <div className="delivery-search-results">{products.map((row)=><button key={row.sku} className={selected?.sku === row.sku ? "active" : ""} onClick={()=>choose(row)}><Box size={16}/><span><b>{row.offerId || row.sku}</b><small>SKU {row.sku} · {row.productName || "未命名商品"}</small></span></button>)}</div>}
        {error && <p className="delivery-error">{error}</p>}
      </div>
      <div className="delivery-inputs">
        <label><span>长 cm</span><input type="number" min="0" value={dimensions.length} onChange={(e)=>setDimensions({...dimensions,length:e.target.value})}/></label>
        <label><span>宽 cm</span><input type="number" min="0" value={dimensions.width} onChange={(e)=>setDimensions({...dimensions,width:e.target.value})}/></label>
        <label><span>高 cm</span><input type="number" min="0" value={dimensions.height} onChange={(e)=>setDimensions({...dimensions,height:e.target.value})}/></label>
        <label><span>商品售价 ₽</span><input type="number" min="0" value={price} onChange={(e)=>setPrice(e.target.value)}/></label>
      </div>
    </section>

    <section className="delivery-summary">
      <article><Box/><span>当前商品<b>{selected ? selected.offerId || selected.sku : "手动查询"}</b></span></article>
      <article><BadgeRussianRuble/><span>计费档位<b>{numericPrice <= 300 ? "售价 ≤ 300 ₽" : "售价 > 300 ₽"}</b></span></article>
      <article><span>包装体积<b>{volume ? `${volume.toFixed(3)} L` : "请填写完整尺寸"}</b><small>长 × 宽 × 高 ÷ 1000</small></span></article>
      <article><span>官方费率生效日<b>{OZON_LOGISTICS_TARIFF_META.effectiveFrom}</b><small>始发区域：{ozonClusterZh(OZON_LOGISTICS_TARIFF_META.origin)}</small></span></article>
    </section>

    <section className="card delivery-table-card">
      <header><div><h2>各区域基础配送费</h2><p>费用为单件商品基础配送参考，不包含佣金、末端附加费及越库费用。</p></div><label><Search size={15}/><input value={destinationQuery} onChange={(e)=>setDestinationQuery(e.target.value)} placeholder="筛选配送区域"/></label></header>
      <table><thead><tr><th>配送区域</th><th>俄文名称</th><th>匹配体积档</th><th>≤ 300 ₽</th><th>&gt; 300 ₽</th><th>当前适用</th></tr></thead>
      <tbody>{destinations.map(({name,tariff})=><tr key={name}><td><b>{ozonClusterZh(name)}</b></td><td>{name}</td><td>{tariff && volume ? `≤ ${tariff.maxLiters} L` : "—"}</td><td>{tariff && volume ? `${tariff.under300.toFixed(2)} ₽` : "—"}</td><td>{tariff && volume ? `${tariff.over300.toFixed(2)} ₽` : "—"}</td><td><strong className="delivery-current">{tariff && volume ? `${(numericPrice <= 300 ? tariff.under300 : tariff.over300).toFixed(2)} ₽` : "等待尺寸"}</strong></td></tr>)}</tbody></table>
    </section>
  </div>;
}
