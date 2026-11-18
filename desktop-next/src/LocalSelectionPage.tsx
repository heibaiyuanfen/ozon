import { useState } from "react";
import type { ReactNode } from "react";
import "./LocalSelectionPage.css";

const candidates: [string, number, number | null, string][] = [
  ["魔术理线扎带 · 10条",249,1.8,"липучки для проводов 10 шт"],
  ["毛毡家具脚垫 · 50片",229,1.5,"накладки фетровые для мебели 50 шт"],
  ["塑料封口夹 · 5个",249,2,"зажимы для пакетов 5 шт"],
  ["替换拉链拉片 · 6个",239,1.5,"съемный язычок для молнии 6 шт"],
  ["喷嘴清洁针 · 10支",249,2,"иглы для прочистки форсунок 10 шт"],
  ["植物固定夹 · 20个",259,2.4,"клипсы для подвязки растений 20 шт"],
  ["透明自粘挂钩 · 6个",219,1.5,"крючки самоклеящиеся прозрачные 6 шт"],
  ["下水口过滤网袋 · 30个",269,2.5,"сеточки для слива раковины 30 шт"],
  ["软管挤压器 · 3个",239,1.8,"выдавливатель для тюбиков 3 шт"],
  ["自粘理线夹 · 6个",259,2.3,"держатель для проводов самоклеящийся 6 шт"],
  ["后备箱折叠收纳箱 · 50 L",1390,null,"органайзер в багажник складной 50 л"],
  ["加固工具包 · 16英寸",2190,null,"сумка для инструментов усиленная с твердым дном"],
  ["衣物真空压缩袋 · 10袋＋泵",1490,null,"вакуумные пакеты для одежды с насосом 10 шт"],
  ["双层数码收纳包",1190,null,"органайзер для проводов двухслойный дорожный"],
  ["车库壁挂工具钩 · 12件",1490,null,"набор крючков для гаража 12 шт"],
  ["断螺栓取出器 · 13件",1590,null,"набор экстракторов болтов 13 шт"],
  ["圣诞树收纳袋 · 150 cm",1290,null,"сумка для хранения елки 150 см"],
  ["轮胎收纳罩 · 4个",1590,null,"чехлы для колес R18 4 шт"],
  ["加厚洗衣网袋 · 8件",1190,null,"мешки для стирки набор 8 шт"],
  ["座椅背收纳袋 · 2件",1690,null,"органайзер на спинку сиденья 2 шт"],
];

const tiers = {
  low: { label: "低于 ₽300 · ROI ≥100%", rate: 26, fees: "", target: 1 },
  high: { label: "高于 ₽1,000 · ROI ≥50%", rate: 45, fees: "130", target: 0.5 },
};

type Tier = keyof typeof tiers;
type Candidate = { id: number; tier: Tier; name: string; price: string; cost: string; keyword: string; fees: string };

function NumberField({ label, value, onChange }: {
  label: string; value: string; onChange: (value: string) => void;
}) {
  return <input aria-label={label} type="number" min="0" step="any" value={value}
    onChange={event => onChange(event.target.value)} />;
}

function readNumber(value: string) {
  const number = Number(value);
  return value.trim() && Number.isFinite(number) && number >= 0 ? number : null;
}

function quote(value: number | null, symbol: string, decimals = 2) {
  return value === null ? "—" : symbol + value.toFixed(decimals);
}

export function LocalSelectionPage() {
  const [tier, setTier] = useState<Tier>("low");
  const [fx, setFx] = useState("12.3419");
  const [rates, setRates] = useState({ low: "26", high: "45" });
  const [qualifiedOnly, setQualifiedOnly] = useState(false);
  const [rows, setRows] = useState<Candidate[]>(() => candidates.map(([name, price, cost, keyword], id) => {
    const tier = id < 10 ? "low" : "high";
    return { id, tier, name, price: String(price), cost: cost?.toString() ?? "", keyword, fees: tiers[tier].fees };
  }));
  const low = tier === "low";
  const exchange = readNumber(fx);
  const rate = readNumber(rates[tier]);
  const validSettings = exchange !== null && exchange > 0 && rate !== null && rate <= 100;
  const target = tiers[tier].target;
  const costLabel = low ? "采购成本 ¥" : "到俄仓成本 ¥";
  const feesLabel = low ? "其他费用 ₽" : "仓储履约 ₽";

  const results = rows.filter(row => row.tier === tier).map(row => {
    const price = readNumber(row.price);
    const cost = readNumber(row.cost);
    const fees = readNumber(row.fees);
    const priceMatches = price !== null && (low ? price > 0 && price < 300 : price > 1000);
    const costMatches = cost !== null && (low ? cost >= 1 && cost <= 3 : cost > 0);
    const sellingFee = validSettings && price !== null ? price * rate / 100 : null;
    const budget = price !== null && sellingFee !== null ? price / (1 + target) - sellingFee : null;
    const purchaseRub = cost !== null && validSettings ? cost * exchange : null;
    const limit = budget === null ? null : low
      ? purchaseRub === null ? null : budget - purchaseRub
      : fees === null || !exchange ? null : (budget - fees) / exchange;
    const total = sellingFee !== null && purchaseRub !== null && fees !== null ? sellingFee + purchaseRub + fees : null;
    const profit = total !== null && price !== null ? price - total : null;
    const roi = profit !== null && total && total > 0 ? profit / total : null;
    const qualified = Boolean(row.name.trim()) && priceMatches && costMatches && roi !== null && roi + 1e-10 >= target;
    const status = qualified ? "达标" : !validSettings ? "检查汇率和费率" : !priceMatches ? "售价不符合" :
      cost !== null && !costMatches ? "成本不符合" : !row.name.trim() || roi === null ? "待补数据" : "ROI 未达标";
    return { row, limit, profit, roi, qualified, status };
  });

  const qualified = results.filter(result => result.qualified).sort((a, b) => (b.roi ?? 0) - (a.roi ?? 0));
  const visible = qualifiedOnly ? qualified.slice(0, 10) : results;

  function update(id: number, key: "name" | "price" | "cost" | "keyword" | "fees", value: string) {
    setRows(current => current.map(row => row.id === id ? { ...row, [key]: value } : row));
  }

  function addCandidate() {
    setQualifiedOnly(false);
    setRows(current => [...current, {
      id: Math.max(...current.map(row => row.id)) + 1, tier, name: "", price: "", cost: "", keyword: "", fees: tiers[tier].fees,
    }]);
  }

  const settings: [string, ReactNode][] = [
    ["汇率（₽/¥）", <NumberField label="汇率" value={fx} onChange={setFx} />],
    [low ? "销售佣金（%）" : "按售价计费合计（%）",
      <NumberField label="费率" value={rates[tier]} onChange={value => setRates(current => ({ ...current, [tier]: value }))} />],
  ];

  return <div className="local-selection">
    <header className="page-header compact">
      <div><h1>Ozon 本土选品</h1><p>FBO 单件测算 · ROI = 单件利润 ÷ 单件全部成本</p></div>
      <button className="dark-button" onClick={addCandidate}>新增候选</button>
    </header>
    <section className="card local-selection-settings" aria-label="测算条件">
      <div className="local-selection-tabs">
        {(Object.keys(tiers) as Tier[]).map(key => <button key={key} aria-pressed={tier === key}
          className={tier === key ? "dark-button" : "outline-button"} onClick={() => setTier(key)}>{tiers[key].label}</button>)}
      </div>
      {settings.map(([label, input]) => <label key={label}>{label}{input}</label>)}
      <label className="local-selection-checkbox"><input type="checkbox" checked={qualifiedOnly}
        onChange={event => setQualifiedOnly(event.target.checked)} />只看达标前 10 款</label>
      <b aria-live="polite">{qualified.length} / {results.length} 款达标</b>
      <p>{low
        ? "整套采购 ¥1–3。其他费用填头程、清关、包装、仓配、广告、退货损耗和税费合计。"
        : "到俄仓成本含货款、头程、清关、包装。费率含佣金、广告、退货损耗和税费，另填仓储履约费。"
      } 预置商品、售价、成本与费率仅供测算，请用实际报价替换。当前编辑仅用于本次页面测算。</p>
    </section>
    <section className="card local-selection-table">
      <table>
        <thead><tr>{["候选商品", "俄语检索词", "售价 ₽", costLabel, feesLabel,
          low ? "其他费用上限 ₽" : "到俄仓成本上限 ¥", "利润 ₽", "ROI", "结果"].map(label => <th key={label}>{label}</th>)}</tr></thead>
        <tbody>{visible.map(({ row, limit, profit, roi, status }) => <tr key={row.id}>
          <td><input aria-label="候选商品" value={row.name} onChange={event => update(row.id, "name", event.target.value)} /></td>
          <td><input aria-label="俄语检索词" value={row.keyword} onChange={event => update(row.id, "keyword", event.target.value)} /></td>
          <td><NumberField label="售价 ₽" value={row.price} onChange={value => update(row.id, "price", value)} /></td>
          <td><NumberField label={costLabel} value={row.cost} onChange={value => update(row.id, "cost", value)} /></td>
          <td><NumberField label={feesLabel} value={row.fees} onChange={value => update(row.id, "fees", value)} /></td>
          <td>{quote(limit === null ? null : Math.floor(limit * 100) / 100, low ? "₽" : "¥")}</td>
          <td>{quote(profit, "₽")}</td>
          <td>{roi === null ? "—" : (roi * 100).toFixed(1) + "%"}</td>
          <td>{status}</td>
        </tr>)}</tbody>
      </table>
      {!visible.length && <p className="empty-note">当前没有达标候选。取消筛选可补填成本和费用。</p>}
    </section>
  </div>;
}
