import { useState } from "react";
import { FileInput, Plus, RotateCcw, Save, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { PurchaseOrderForm, PurchaseOrderItem, PurchaseOrderSummary } from "./PurchaseOrderPage";
import { products, refreshProductPrice } from "./bridge";
import type { DateRange, ProductRow } from "./types";
import { officialLogisticsTariff } from "./ozon-tariffs";
import "./LocalSelectionPage.css";
import "./new-product-profit.css";

const fields = {
  price: ["卖家售价 ₽", ""],
  cost: ["采购成本 ¥", ""],
  cnyRate: ["人民币汇率 ₽/¥", "14"],
  baseDelivery: ["基础物流费 ₽", ""],
  commission: ["佣金 %", ""],
  subsidy: ["页面价补贴 %", "40"],
  deliveryRate: ["配送附加费 %", "1"],
  acquiring: ["收单费 %", "1.5"],
  damage: ["货损 %", "3"],
  ad: ["广告 %", "10"],
  stars: ["星星 %", "1.5"],
  vat: ["增值税 %", "0"],
  tax: ["利润税 %", "3"],
  payoutFee: ["回款费 %", "10"],
  length: ["包装长 cm", ""],
  width: ["包装宽 cm", ""],
  height: ["包装高 cm", ""],
  weight: ["包装重 kg", ""],
  count: ["每箱件数", ""],
  cartonLength: ["外箱长 cm", ""],
  cartonWidth: ["外箱宽 cm", ""],
  cartonHeight: ["外箱高 cm", ""],
  cartonTare: ["空箱重 kg", "1"],
  cartonWeight: ["整箱毛重手填 kg（选填）", ""],
  shippingQuote: ["头程报价 USD/kg", "2"],
  usdRate: ["美元汇率 ₽/USD", "93.8"],
} as const;

type Field = keyof typeof fields;
type Input = Record<Field, string>;
type Product = { name: string; competitor: string; supplier: string };
type SavedCalculation = { id: string; product: Product; input: Input; autoDelivery: boolean };
const emptyProduct: Product = { name: "", competitor: "", supplier: "" };
const defaults = Object.fromEntries(Object.entries(fields).map(([key, [, value]]) => [key, value])) as Input;

function purchaseItemInput(item: PurchaseOrderItem): Input {
  return {
    ...defaults,
    cost: item.unitPrice ?? "",
    length: item.packageLengthCm ?? "",
    width: item.packageWidthCm ?? "",
    height: item.packageHeightCm ?? "",
    weight: item.unitWeightKg ?? "",
    count: item.unitsPerCarton ?? "",
    cartonLength: item.cartonLengthCm ?? "",
    cartonWidth: item.cartonWidthCm ?? "",
    cartonHeight: item.cartonHeightCm ?? "",
    cartonWeight: item.cartonWeightKg ?? "",
  };
}

const groups: [string, Field[]][] = [
  ["售价与采购", ["price", "cost", "cnyRate", "baseDelivery", "commission", "subsidy", "deliveryRate"]],
  ["包装与头程", ["length", "width", "height", "weight", "count", "cartonLength", "cartonWidth", "cartonHeight", "cartonTare", "cartonWeight", "shippingQuote", "usdRate"]],
  ["运营费率", ["acquiring", "damage", "ad", "stars", "vat", "tax", "payoutFee"]],
];

function read(input: Input, key: Field, min = 0, max = Infinity) {
  const value = Number(input[key]);
  return input[key].trim() && Number.isFinite(value) && value >= min && value <= max ? value : null;
}

export function calculateNewProduct(input: Input, autoDelivery = !input.baseDelivery.trim()) {
  const price = read(input, "price", 0.01);
  const cost = read(input, "cost");
  const cnyRate = read(input, "cnyRate", 0.01);
  const purchase = cost !== null && cnyRate !== null ? cost * cnyRate : null;
  const quantity = read(input, "count", 1);
  const count = quantity !== null && Number.isInteger(quantity) ? quantity : null;
  const weight = read(input, "weight", 0.001);
  const tare = read(input, "cartonTare");
  const cartonWeight = input.cartonWeight.trim() ? read(input, "cartonWeight", 0.001)
    : count !== null && weight !== null && tare !== null ? count * weight + tare : null;
  const rate = read(input, "shippingQuote");
  const usdRate = read(input, "usdRate", 0.01);
  const shipping = count !== null && cartonWeight !== null && rate !== null && usdRate !== null
    ? rate * cartonWeight / count * usdRate : null;

  function volume(keys: Field[], divisor: number) {
    const dimensions = keys.map(key => read(input, key, 0.001));
    return dimensions.every(value => value !== null)
      ? dimensions.reduce<number>((product, value) => product * (value ?? 0), 1) / divisor : null;
  }

  const liters = volume(["length", "width", "height"], 1000);
  const tariff = price !== null && liters !== null ? officialLogisticsTariff("Екатеринбург", liters) : null;
  const quotedDelivery = tariff && !tariff.isFallback && price !== null
    ? price <= 300 ? tariff.under300 : tariff.over300 : null;
  const cartonVolume = volume(["cartonLength", "cartonWidth", "cartonHeight"], 1_000_000);
  const density = cartonVolume !== null && cartonWeight !== null ? cartonWeight / cartonVolume : null;

  function fee(key: Field) {
    const rate = read(input, key, 0, 100);
    return price === null || rate === null ? null : price * rate / 100;
  }

  const discount = fee("subsidy");
  const pagePrice = price !== null && discount !== null ? price - discount : null;
  const baseDelivery = autoDelivery ? quotedDelivery : read(input, "baseDelivery");
  const surcharge = fee("deliveryRate");
  const delivery = baseDelivery !== null && surcharge !== null ? baseDelivery + surcharge : null;
  const deductions = [fee("commission"), delivery, fee("acquiring"), fee("damage"), fee("ad"), fee("stars")];
  const payout = price !== null && deductions.every(value => value !== null)
    ? price - deductions.reduce<number>((total, value) => total + (value ?? 0), 0) : null;
  const payoutRate = read(input, "payoutFee", 0, 100);
  const payoutFee = payout !== null && payoutRate !== null ? payout * payoutRate / 100 : null;
  const vat = fee("vat");
  const tax = fee("tax");
  const invested = purchase !== null && shipping !== null ? purchase + shipping : null;
  const profit = payout !== null && invested !== null && vat !== null && tax !== null && payoutFee !== null
    ? payout - invested - vat - tax - payoutFee : null;
  return {
    pagePrice, purchase, shipping, delivery, payout, vat, tax, payoutFee, profit, liters, density, cartonWeight, quotedDelivery,
    commission: deductions[0], acquiring: deductions[2], damage: deductions[3], ad: deductions[4], stars: deductions[5],
    margin: profit !== null && price !== null ? profit / price : null,
    payoutRatio: payout !== null && price !== null ? payout / price : null,
    roi: profit !== null && invested !== null && invested > 0 ? profit / invested : null,
  };
}

function format(value: number | null, unit: string) {
  if (value === null) return "—";
  if (unit === "%") return (value * 100).toFixed(2) + "%";
  return value.toFixed(2) + " " + unit;
}

export function NewProductProfitPage({ shopId, range }: { shopId: string; range: DateRange }) {
  const storageKey = "ozon:new-product-profit:" + shopId;
  const [stored] = useState(() => {
    try {
      const value: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "[]");
      if (!Array.isArray(value)) throw new Error();
      for (const record of value) {
        if (typeof record?.id !== "string" || typeof record.autoDelivery !== "boolean" ||
          Object.keys(emptyProduct).some(key => typeof record.product?.[key] !== "string") ||
          Object.keys(defaults).some(key => typeof record.input?.[key] !== "string")) throw new Error();
      }
      return { records: value as SavedCalculation[], readable: true };
    } catch {
      return { records: [] as SavedCalculation[], readable: false };
    }
  });
  const [records, setRecords] = useState(stored.records);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [input, setInput] = useState<Input>(defaults);
  const [autoDelivery, setAutoDelivery] = useState(true);
  const [product, setProduct] = useState<Product>(emptyProduct);
  const [message, setMessage] = useState(stored.readable ? "" : "无法读取已保存的测算记录。");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [orders, setOrders] = useState<PurchaseOrderSummary[] | null>(null);
  const [orderId, setOrderId] = useState("");
  const [order, setOrder] = useState<PurchaseOrderForm | null>(null);
  const [itemIndex, setItemIndex] = useState("");
  const [loadingOrder, setLoadingOrder] = useState(false);
  const [catalog, setCatalog] = useState<ProductRow[] | null>(null);
  const [catalogQuery, setCatalogQuery] = useState("");
  const [catalogSku, setCatalogSku] = useState("");
  const [catalogBusy, setCatalogBusy] = useState(false);
  const catalogItem = catalog?.find(row => row.sku === catalogSku);
  const r = calculateNewProduct(input, autoDelivery);

  function persist(next: SavedCalculation[]) {
    if (!stored.readable) return false;
    try {
      localStorage.setItem(storageKey, JSON.stringify(next));
      setRecords(next);
      return true;
    } catch {
      setMessage("保存失败，本机存储不可用。");
      return false;
    }
  }

  function openRecord(record?: SavedCalculation) {
    setActiveId(record?.id ?? null);
    setProduct(record?.product ?? emptyProduct);
    setInput(record?.input ?? defaults);
    setAutoDelivery(record?.autoDelivery ?? true);
    setConfirmDelete(false);
    setMessage(stored.readable ? "" : "无法读取已保存的测算记录。");
  }

  function save() {
    if (!product.name.trim()) return;
    const record = { id: activeId ?? crypto.randomUUID(), product: { ...product, name: product.name.trim() }, input, autoDelivery };
    const next = activeId ? records.map(existing => existing.id === activeId ? record : existing) : [record, ...records];
    if (persist(next)) {
      setActiveId(record.id);
      setProduct(record.product);
      setConfirmDelete(false);
      setMessage("测算已保存。");
    }
  }

  function remove() {
    if (!activeId || !persist(records.filter(record => record.id !== activeId))) return;
    openRecord();
    setMessage("测算已删除。");
  }

  async function searchCatalog(query: string) {
    setOrders(null);
    setCatalog([]);
    setCatalogSku("");
    setCatalogBusy(true);
    setMessage("");
    try {
      setCatalog(await products(range, query.trim()));
    } catch (error) {
      setCatalog(null);
      setMessage("读取商品中心失败：" + String(error));
    } finally {
      setCatalogBusy(false);
    }
  }

  async function importCatalogItem(item: ProductRow) {
    setCatalogBusy(true);
    let price = "";
    try {
      const quote = await refreshProductPrice(item.sku);
      if (quote.currencyCode.toUpperCase() === "RUB" && Number.isFinite(quote.price) && quote.price > 0) {
        price = String(quote.price);
      }
    } catch {
      // Local product data is available even when the seller price cannot be fetched.
    } finally {
      setCatalogBusy(false);
    }
    openRecord();
    setProduct({ ...emptyProduct, name: item.name.trim() || item.offerId || item.sku });
    setInput({
      ...defaults,
      price,
      cost: String(item.unitCost ?? ""),
      length: String(item.lengthCm ?? ""),
      width: String(item.widthCm ?? ""),
      height: String(item.heightCm ?? ""),
      weight: String(item.weightKg ?? ""),
    });
    setCatalog(null);
    setMessage(`已导入 SKU ${item.sku}。${price ? "" : "未取得卢布卖家售价，请手动填写。"}请补充缺失成本、佣金和装箱数据。`);
  }

  async function openPurchaseOrders() {
    setCatalog(null);
    setLoadingOrder(true);
    setMessage("");
    setOrders([]);
    setOrderId("");
    setOrder(null);
    setItemIndex("");
    try {
      const result = await invoke<{ orders: PurchaseOrderSummary[] }>("purchase_order_command",
        { command: "list", id: null, payload: null });
      setOrders(result.orders);
      if (!result.orders.length) setMessage("暂无已保存的采购单。");
    } catch (error) {
      setOrders(null);
      setMessage("读取采购单失败：" + String(error));
    } finally {
      setLoadingOrder(false);
    }
  }

  async function choosePurchaseOrder(id: string) {
    setOrderId(id);
    setOrder(null);
    setItemIndex("");
    setMessage("");
    if (!id) return;
    setLoadingOrder(true);
    try {
      setOrder(await invoke<PurchaseOrderForm>("purchase_order_command",
        { command: "detail", id: Number(id), payload: null }));
    } catch (error) {
      setMessage("读取采购单明细失败：" + String(error));
    } finally {
      setLoadingOrder(false);
    }
  }

  const selectedItem = itemIndex === "" ? undefined : order?.items[Number(itemIndex)];

  function importPurchaseItem() {
    if (!selectedItem || !order) return;
    openRecord();
    setProduct({ ...emptyProduct, name: selectedItem.productName?.trim() || selectedItem.sku?.trim()
      || order.title?.trim() || "采购商品" });
    setInput(purchaseItemInput(selectedItem));
    setOrders(null);
    setMessage(`已从采购单 ${order.orderNo} 带入第 ${Number(itemIndex) + 1} 条商品。填写售价和相关费率后可测算。`);
  }

  const metrics: [string, number | null, string][] = [
    ["利润", r.profit, "₽"], ["利润率", r.margin, "%"], ["采购＋头程 ROI", r.roi, "%"],
    ["回款", r.payout, "₽"], ["回款率", r.payoutRatio, "%"], ["页面价", r.pagePrice, "₽"],
    ["采购价", r.purchase, "₽"], ["单件头程", r.shipping, "₽"], ["配送费", r.delivery, "₽"],
    ["包装体积", r.liters, "L"], ["整箱毛重", r.cartonWeight, "kg"], ["外箱密度", r.density, "kg/m³"],
    ["佣金", r.commission, "₽"], ["收单费", r.acquiring, "₽"], ["货损", r.damage, "₽"],
    ["广告", r.ad, "₽"], ["星星", r.stars, "₽"], ["增值税", r.vat, "₽"],
    ["利润税", r.tax, "₽"], ["回款费", r.payoutFee, "₽"],
  ];

  return <div className="local-selection new-product-profit">
    <header className="page-header compact"><div><h1>新品利润测算</h1>
      <p>宜智测算口径 · 单件利润、回款与投入回报</p></div>
      <div className="profit-actions">
        <button type="button" className="profit-button" onClick={() => openRecord()}><Plus aria-hidden="true" />新增</button>
        <button type="button" className="profit-button" disabled={loadingOrder || catalogBusy}
          onClick={() => void openPurchaseOrders()}><FileInput aria-hidden="true" />从采购单载入</button>
        <button type="button" className="profit-button" disabled={loadingOrder || catalogBusy}
          onClick={() => { setCatalogQuery(""); void searchCatalog(""); }}><FileInput aria-hidden="true" />从商品中心导入</button>
        <button type="button" className="profit-button profit-button-primary" disabled={!stored.readable || !product.name.trim()} onClick={save}><Save aria-hidden="true" />保存</button>
        <button type="button" className="profit-button profit-button-danger" disabled={!activeId} onClick={() => setConfirmDelete(true)}><Trash2 aria-hidden="true" />删除</button>
      </div>
    </header>
    {catalog !== null && <section className="card local-selection-settings" aria-label="从商品中心导入">
      <form className="profit-actions" onSubmit={event => { event.preventDefault(); void searchCatalog(catalogQuery); }}>
        <label>搜索商品<input value={catalogQuery} placeholder="名称、货号、SKU 或条码"
          disabled={catalogBusy} onChange={event => setCatalogQuery(event.target.value)} /></label>
        <button type="submit" className="profit-button" disabled={catalogBusy}>搜索</button>
      </form>
      <label>商品<select value={catalogSku} disabled={catalogBusy || !catalog.length}
        onChange={event => setCatalogSku(event.target.value)}>
        <option value="">{catalogBusy ? "处理中…" : catalog.length ? "请选择一款商品" : "没有匹配的商品"}</option>
        {catalog.map(row => <option key={row.sku} value={row.sku}>{row.name || row.offerId || row.sku} · {row.offerId} · SKU {row.sku}</option>)}
      </select></label>
      <button type="button" className="profit-button profit-button-primary" disabled={catalogBusy || !catalogItem}
        onClick={() => { if (catalogItem) void importCatalogItem(catalogItem); }}>带入新测算</button>
      <button type="button" className="profit-button" disabled={catalogBusy} onClick={() => setCatalog(null)}>取消</button>
      <p>导入会建立一条新测算，请先保存当前编辑。</p>
    </section>}
    {orders !== null && <section className="card local-selection-settings" aria-label="从采购单载入">
      <label>采购单<select value={orderId} disabled={loadingOrder}
        onChange={event => void choosePurchaseOrder(event.target.value)}>
        <option value="">{loadingOrder ? "读取中…" : "请选择采购单"}</option>
        {orders.map(row => <option key={row.id} value={row.id}>{row.orderNo} · {row.title} · {row.itemCount} 项</option>)}
      </select></label>
      <label>商品明细<select value={itemIndex} disabled={loadingOrder || !order?.items.length}
        onChange={event => setItemIndex(event.target.value)}>
        <option value="">请选择一款商品</option>
        {order?.items.map((item, index) => <option key={index} value={index}>
          {index + 1}. {item.productName || "未命名"}{item.sku ? ` · SKU ${item.sku}` : ""}{item.unitPrice ? ` · ¥${item.unitPrice}` : ""}
        </option>)}
      </select></label>
      <button type="button" className="profit-button profit-button-primary" disabled={loadingOrder || !selectedItem} onClick={importPurchaseItem}>
        带入新测算
      </button>
      <button type="button" className="profit-button" disabled={loadingOrder} onClick={() => setOrders(null)}>取消</button>
      <p>所选商品会建立一条新测算，请先保存当前编辑。</p>
    </section>}
    <section className="card local-selection-settings">
      <label>已保存的新品测算<select value={activeId ?? ""} onChange={event =>
        openRecord(records.find(record => record.id === event.target.value))}>
        <option value="">新建测算</option>
        {records.map(record => {
          const result = calculateNewProduct(record.input, record.autoDelivery);
          return <option key={record.id} value={record.id}>
            {record.product.name} · 利润率 {format(result.margin, "%")} · ROI {format(result.roi, "%")}
          </option>;
        })}
      </select></label>
      {message && <span role="status">{message}</span>}
      {confirmDelete && <div role="alert">
        删除「{records.find(record => record.id === activeId)?.product.name}」？
        <button type="button" className="profit-button" onClick={() => setConfirmDelete(false)}>取消</button>
        <button type="button" className="profit-button profit-button-danger-solid" onClick={remove}>确认删除</button>
      </div>}
    </section>
    <section className="card local-selection-settings">
      {([["name", "产品名称"], ["competitor", "竞品链接"], ["supplier", "采购链接"]] as const).map(([key, label]) =>
        <label key={key}>{label}<input type="text" value={product[key]}
          onChange={event => setProduct(current => ({ ...current, [key]: event.target.value }))} /></label>)}
    </section>
    {groups.map(([title, keys]) => <section className="card local-selection-settings" key={title}>
      <h2 style={{ flexBasis: "100%", margin: 0 }}>{title}</h2>
      {keys.map(key => <label key={key}>{fields[key][0]}<input type="number" min="0" step="any"
        value={key === "baseDelivery" && autoDelivery ? r.quotedDelivery ?? "" : input[key]}
        onChange={event => {
          const value = event.target.value;
          if (key === "baseDelivery") setAutoDelivery(false);
          setInput(current => ({ ...current, [key]: value }));
        }} /></label>)}
      {keys.includes("cost") && <label>采购价 ₽<input readOnly value={format(r.purchase, "₽")} /></label>}
      {keys.includes("shippingQuote") && <label>单件头程 ₽<input readOnly value={format(r.shipping, "₽")} /></label>}
      {keys.includes("baseDelivery") && <div>
        <div>莫斯科 → 叶卡捷琳堡 · {r.quotedDelivery === null ? "填写有效售价与包装三边以查询线路费用" : format(r.quotedDelivery, "₽")}</div>
        {!autoDelivery && <button type="button" className="profit-button"
          onClick={() => setAutoDelivery(true)}><RotateCcw aria-hidden="true" />恢复自动费用</button>}
      </div>}
    </section>)}
    <section className="card local-selection-settings" aria-label="测算结果" aria-live="polite">
      {metrics.map(([label, value, unit]) => <label key={label}>{label}<b>{format(value, unit)}</b></label>)}
      <p>ROI 按利润 ÷（采购价＋头程）。按售价计提运营费与税费，回款费按回款计提。</p>
      <p>采购价 = 人民币采购成本 × 人民币兑卢布汇率。单件头程 = 美元报价 × 整箱毛重 ÷ 每箱件数 × 美元兑卢布汇率。</p>
    </section>
  </div>;
}
