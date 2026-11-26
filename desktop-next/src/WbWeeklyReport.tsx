import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Order = { id: string; day: string; status: string; raw: Record<string, unknown>; warehouseId?: string | number | null };
type Warehouse = { id: string; raw: Record<string, unknown> };
type Ads = {day: string; spend: number; attributedSales: number; detail?: { orders?: number | null } | null};
type Cost = { purchase: string; first: string; last: string };
type Rule = { kind: "domestic" | "overseas"; base: string; rate: string; handling: string; outbound: string };
type Receipt = { amount: string; reference: string; weight: string };
type Config = { rules: Record<string, Rule>; receipts: Record<string, Receipt> };
type Currency = "CNY" | "RUB";

const emptyRule: Rule = {kind: "domestic", base: "", rate: "", handling: "", outbound: ""};
const emptyReceipt: Receipt = { amount: "", reference: "", weight: "" };
const number = (text?: string) => text?.trim() && Number.isFinite(Number(text)) && Number(text) >= 0 ? Number(text) : null;
const warehouseKey = (order: Order) => String(order.raw.warehouseId || order.warehouseId || "");
const cancelled = (order: Order) => order.status === "cancelled" || order.status === "returning";

export function WbWeeklyReport({shopId, currency, today, orderRub, exchange}: {
  shopId: string; currency: Currency; today: string; orderRub: (order: Order) => number; exchange: number;
}) {
  const storageKey = `wb-weekly-report-v1:${shopId}:${currency}`;
  const [data, setData] = useState<{orders: Order[]; warehouses: Warehouse[]}>({orders: [], warehouses: []});
  const [ads, setAds] = useState<Ads[]>([]);
  const [costs, setCosts] = useState<Record<string, Cost>>({});
  const [config, setConfig] = useState<Config>(() => {
    try { return JSON.parse(localStorage.getItem(storageKey) || "null") ?? {rules: {}, receipts: {}}; }
    catch { return {rules: {}, receipts: {}}; }
  });
  const [offset, setOffset] = useState(0);
  const [warehouse, setWarehouse] = useState("");
  const [orderId, setOrderId] = useState("");
  const [busy, setBusy] = useState(true);
  const [errors, setErrors] = useState<string[]>([]);

  const date = new Date(`${today}T12:00:00Z`);
  date.setUTCDate(date.getUTCDate() - (date.getUTCDay() + 6) % 7 - offset * 7);
  const days = Array.from({length: 7}, (_, index) => {
    const day = new Date(date);
    day.setUTCDate(day.getUTCDate() + index);
    return day.toISOString().slice(0, 10);
  }).filter(day => day <= today);
  const from = days[0], to = days[days.length - 1];
  const ops = <T,>(action: string, payload?: object) =>
    invoke<T>("wb_operations", {action, payload: {shopId, ...payload}});

  async function load(syncAds = false) {
    setBusy(true);
    setErrors([]);
    const result = await Promise.allSettled([
      ops<typeof data>("list"),
      ops<Ads[]>(syncAds ? "ads_sync" : "ads_list", syncAds ? {from, to} : undefined),
      invoke<{costs?: Record<string, Cost>}>("wb_pricing", {action: "load", payload: {shopId}}),
    ]);
    if (result[0].status === "fulfilled") setData(result[0].value);
    else setData({orders: [], warehouses: []});
    if (result[1].status === "fulfilled") setAds(result[1].value);
    else setAds([]);
    if (result[2].status === "fulfilled") setCosts(result[2].value.costs ?? {});
    else setCosts({});
    setErrors(result.flatMap((item, index) => item.status === "rejected" ?
      [`${["订单", "广告", "商品成本"][index]}：${String(item.reason)}`] : []));
    setBusy(false);
  }

  useEffect(() => { void load(); }, [shopId]);

  const orders = data.orders.filter(row => row.day >= from && row.day <= to);
  const signed = orders.filter(row => row.status === "received");
  const warehouses = new Map(data.warehouses.map(row => [
    String(row.raw.id ?? row.id), String(row.raw.name ?? row.id),
  ]));
  for (const row of orders) {
    const key = warehouseKey(row);
    if (key && !warehouses.has(key)) warehouses.set(key, `仓库 ${key}`);
  }
  const byDay = new Map(ads.map(row => [row.day, row]));
  const covered = days.filter(day => byDay.has(day));
  const adDays = covered.map(day => byDay.get(day)!);
  const sum = (values: number[]) => values.reduce((total, value) => total + value, 0);
  const adSpend = covered.length === days.length ? sum(adDays.map(row => row.spend)) : null;
  const adSales = covered.length === days.length ? sum(adDays.map(row => row.attributedSales)) : null;
  const adOrders = covered.length === days.length && adDays.every(row => row.detail?.orders != null)
    ? sum(adDays.map(row => row.detail!.orders!)) : null;
  const saleValues = orders.filter(row => !cancelled(row)).map(orderRub);
  const sales = saleValues.every(value => value > 0) ? sum(saleValues) / (currency === "CNY" ? exchange : 1) : null;
  const money = (value: number | null) => value === null ? "待确认" :
    `${currency === "CNY" ? "¥" : "₽"}${value.toLocaleString("zh-CN", {minimumFractionDigits: 2, maximumFractionDigits: 2})}`;
  const ratio = (numerator: number | null, denominator: number | null) =>
    numerator === null || denominator === null || denominator <= 0 ? "—" : `${(numerator / denominator * 100).toFixed(2)}%`;

  const paid = signed.filter(row => config.receipts[row.id]);
  const cnyFactor = currency === "CNY" ? 1 : exchange;
  const lines = paid.map(row => {
    const receipt = config.receipts[row.id];
    const rule = config.rules[warehouseKey(row)];
    const nmId = row.raw.nmId ?? row.raw.nm_id ?? row.raw.nmID;
    const cost = costs[`${rule?.kind}:${nmId}`];
    const purchase = number(cost?.purchase);
    const freight = rule?.kind === "domestic"
      ? [number(rule.base), number(rule.rate), number(receipt.weight)]
      : [number(cost?.first), number(rule?.handling), number(rule?.outbound), number(cost?.last)];
    const shipping = !rule || freight.some(value => value === null) ? null : rule.kind === "domestic"
      ? freight[0]! + freight[1]! * freight[2]! : sum(freight as number[]);
    return {row, receipt, rule, purchase: purchase === null ? null : purchase * cnyFactor,
      shipping: shipping === null ? null : shipping * cnyFactor};
  });
  const complete = lines.length > 0 && lines.every(line => line.purchase !== null && line.shipping !== null);
  const net = sum(lines.map(line => Number(line.receipt.amount)));
  const purchase = complete ? sum(lines.map(line => line.purchase!)) : null;
  const shipping = complete ? sum(lines.map(line => line.shipping!)) : null;
  const profit = purchase !== null && shipping !== null && adSpend !== null ? net - purchase - shipping - adSpend : null;

  const rule = config.rules[warehouse] ?? emptyRule;
  const receipt = config.receipts[orderId] ?? emptyReceipt;
  function save(configNext: Config) {
    try {
      localStorage.setItem(storageKey, JSON.stringify(configNext));
      setConfig(configNext);
    } catch {
      setErrors(["无法写入本机周报设置"]);
    }
  }
  function updateRule(field: keyof Rule, value: string) {
    save({...config, rules: {...config.rules, [warehouse]: {...rule, [field]: value}}});
  }
  const receiptRule = config.rules[warehouseKey(signed.find(row => row.id === orderId) ?? {id:"", day:"", status:"", raw:{}})];

  return <>
    <section className="wbo-panel">
      <div className="wbo-heading"><div><h2>周度经营汇总</h2><span>{from} 至 {to} · 莫斯科日期 · {currency}</span></div>
        <div className="wbo-actions">
          <button onClick={() => {setOffset(offset + 1); setOrderId("");}}>上一周</button>
          <button disabled={!offset} onClick={() => {setOffset(offset - 1); setOrderId("");}}>下一周</button>
          <button disabled={busy} onClick={() => void load()}>刷新缓存</button>
          <button disabled={busy} onClick={() => void load(true)}>同步本周广告</button>
        </div>
      </div>
      {busy && <p role="status">正在读取周报数据…</p>}
      {errors.map(error => <p key={error} className="wbo-error" role="alert">{error}</p>)}
      <dl className="wbo-metric-row">{([
        ["广告费", money(adSpend)], ["广告订单", adOrders ?? "待确认"], ["总订单", orders.length],
        ["ACOS", ratio(adSpend, adSales)], ["TACOS", ratio(adSpend, sales)], ["已回款周利润", money(profit)],
      ] as const).map(([label, value]) => <div key={label}><dt>{label}</dt><dd title={String(value)}>{value}</dd></div>)}</dl>
      <p className="wbo-muted">广告覆盖 {covered.length}/{days.length} 天。ACOS 按广告归因销售额计算，TACOS 按扣除取消与拒收后的下单销售额计算。金额换算使用 ¥1 = ₽{exchange}。</p>
    </section>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>仓库成本规则</h2><span>费用按件填报，币种 CNY</span></div>
      <div className="wbo-toolbar">
        <label>仓库<select value={warehouse} onChange={event => setWarehouse(event.target.value)}>
          <option value="">选择仓库</option>{[...warehouses].map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </select></label>
        {warehouse && <>
          <label>发货类型<select value={rule.kind} onChange={event => updateRule("kind", event.target.value)}>
            <option value="domestic">国内仓</option><option value="overseas">海外仓</option>
          </select></label>
          {(rule.kind === "domestic" ? [["base", "公式固定费"], ["rate", "公式每公斤费率"]] :
            [["handling", "操作费"], ["outbound", "出仓费"]]).map(([field, label]) =>
            <label key={field}>{label}<input type="number" min="0" step="0.01"
              onFocus={() => {if (!config.rules[warehouse]) updateRule("kind", rule.kind);}}
              value={rule[field as keyof Rule]} onChange={event => updateRule(field as keyof Rule, event.target.value)}/></label>)}
        </>}
      </div>
      <p className="wbo-muted">采购成本、海外仓头程和后段读取「价格与利润监控」的对应商品成本。国内仓全程运费 = 固定费 + 每公斤费率 × 计费重量，无需再扣头程费用。</p>
    </section>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>回款确认与利润明细</h2><span>已签收 {signed.length} 单 · 已确认回款 {paid.length} 单</span></div>
      <form className="wbo-toolbar" key={orderId} onSubmit={event => {
        event.preventDefault();
        const data = new FormData(event.currentTarget);
        const entry = {amount: String(data.get("amount")), reference: String(data.get("reference")).trim(), weight: String(data.get("weight") ?? "")};
        if (!orderId || !entry.reference || !(Number(entry.amount) > 0) || !Number.isFinite(Number(entry.amount))) return;
        save({...config, receipts: {...config.receipts, [orderId]: entry}});
      }}>
        <label>已签收订单<select value={orderId} onChange={event => setOrderId(event.target.value)}>
          <option value="">选择订单</option>{signed.map(row => <option key={row.id} value={row.id}>
            {row.id} · {warehouses.get(warehouseKey(row)) ?? "仓库未关联"}
          </option>)}
        </select></label>
        {orderId && <>
          <label>实收净额 {currency}<input required name="amount" type="number" min="0.01" step="0.01" defaultValue={receipt.amount}/></label>
          <label>回款凭证编号<input required name="reference" defaultValue={receipt.reference}/></label>
          {receiptRule?.kind !== "overseas" && <label>计费重量 kg<input name="weight" type="number" min="0" step="0.001" defaultValue={receipt.weight}/></label>}
          <button className="primary">保存回款确认</button>
          {config.receipts[orderId] && <button type="button" onClick={() => {
            const remaining = {...config.receipts};
            delete remaining[orderId];
            save({...config, receipts: remaining});
            setOrderId("");
          }}>撤销确认</button>}
        </>}
      </form>
      <p className="wbo-muted">按订单创建周归属。实收净额填写已到账、扣除平台扣款后且尚未扣除这里填报成本及周广告费的金额。利润 = 实收净额 − 采购成本 − 仓配运费 − 全周广告费。</p>
      <div className="wbo-table-wrap"><table>
        <thead><tr><th>订单</th><th>仓库</th><th>凭证</th><th>实收净额</th><th>采购成本</th><th>仓配运费</th></tr></thead>
        <tbody>{lines.map(({row, receipt, purchase, shipping}) => <tr key={row.id}>
          <td>{row.id}</td><td>{warehouses.get(warehouseKey(row)) ?? "待关联"}</td><td>{receipt.reference}</td>
          <td>{money(Number(receipt.amount))}</td><td>{money(purchase)}</td><td>{money(shipping)}</td>
        </tr>)}</tbody>
        {lines.length > 0 && <tfoot><tr><th colSpan={3}>已确认合计</th><td>{money(net)}</td><td>{money(purchase)}</td><td>{money(shipping)}</td></tr></tfoot>}
      </table></div>
      {!paid.length && <p>本周暂无已签收且已确认回款的订单。</p>}
    </section>
  </>;
}
