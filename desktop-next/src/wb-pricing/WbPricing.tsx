import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
type Cost = {
  mode: string;
  purchase: string;
  first: string;
  storage: string;
  last: string;
  domestic: string;
  international: string;
  customs: string;
  other: string;
  commission: string;
  advertising: string;
  loss: string;
  exchange: string;
  target: string;
};
type Goods = {
  nmID: number;
  vendorCode: string;
  currencyIsoCode4217: string;
  discount: number;
  clubDiscount?: number;
  editableSizePrice?: boolean;
  sizes: {
    sizeID: number;
    price: number;
    discountedPrice: number;
    clubDiscountedPrice?: number;
    techSizeName: string;
  }[];
};
const priceFields = {
  price: "原价",
  discount: "普通折扣",
  clubDiscount: "WB Club 会员折扣",
};
type PriceField = keyof typeof priceFields;
const defaults: Cost = {
  mode: "overseas",
  purchase: "",
  first: "0",
  storage: "0",
  last: "0",
  domestic: "0",
  international: "0",
  customs: "0",
  other: "0",
  commission: "",
  advertising: "0",
  loss: "0",
  exchange: "",
  target: "15",
};
const labels: Record<string, string> = {
  purchase: "采购成本 CNY",
  first: "单件头程 CNY",
  storage: "单件仓储 CNY",
  last: "尾程配送 CNY",
  domestic: "国内段 CNY",
  international: "国际段 CNY",
  customs: "清关 CNY",
  other: "包装及其他 CNY",
  commission: "佣金 %",
  advertising: "广告 %",
  loss: "损耗预留 %",
  exchange: "1 CNY 对应 RUB",
  target: "目标利润率 %",
};
export function WbPricing({ shopId }: { shopId: string }) {
  const [goods, setGoods] = useState<Goods[]>([]),
    [costs, setCosts] = useState<Record<string, Cost>>({}),
    [mode, setMode] = useState("overseas"),
    [query, setQuery] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [ready, setReady] = useState(false),
    [updated, setUpdated] = useState("");
  const [repricing, setRepricing] = useState<{ goods: Goods; sizeID: number } | null>(null),
    [field, setField] = useState<PriceField>("price"),
    [newPrice, setNewPrice] = useState(""),
    [task, setTask] = useState<number | null>(null);
  const [edit, setEdit] = useState<number | null>(null),
    [draft, setDraft] = useState<Cost>(defaults);
  const api = <T,>(action: string, extra: Record<string, unknown> = {}) =>
    invoke<T>("wb_pricing", { action, payload: { shopId, ...extra } });
  useEffect(() => {
    let alive = true;
    api<{
      catalog?: { goods: Goods[]; updatedAt: string };
      costs?: Record<string, Cost>;
      lastTask?: { data?: { id?: number } };
    }>("load")
      .then((d) => {
        if (alive) {
          setGoods(d.catalog?.goods ?? []);
          setUpdated(d.catalog?.updatedAt ?? "");
          setCosts(d.costs ?? {});
          setTask(d.lastTask?.data?.id ?? null);
          setReady(true);
        }
      })
      .catch((e) => {
        if (alive) setMessage(String(e));
      });
    return () => {
      alive = false;
    };
  }, [shopId]);
  async function sync() {
    setBusy(true);
    try {
      const d = await api<{ goods: Goods[]; updatedAt: string }>("sync");
      setGoods(d.goods);
      setUpdated(d.updatedAt);
      setMessage("商品价格已同步");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (edit == null) return;
    const next = { ...costs, [`${mode}:${edit}`]: draft };
    setBusy(true);
    try {
      await api("save", { costs: next });
      setCosts(next);
      setEdit(null);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function submit() {
    if (!repricing || busy || newPrice === "") return;
    const { goods: product, sizeID } = repricing;
    setBusy(true);
    setMessage("");
    try {
      const r = await api<{ data?: { id?: number } }>("submit", {
        nmID: product.nmID,
        field,
        value: Number(newPrice),
        ...(field === "price" && product.editableSizePrice ? { sizeID } : {}),
      });
      const id = r.data?.id;
      if (id && id > 0) setTask(id);
      setMessage(
        id && id > 0
          ? `${priceFields[field]}已提交，等待 WB 处理。任务 #${id}，生效后可重新读取当前价格。`
          : "WB 已响应，但未返回有效任务编号。请重新读取当前价格或在 WB 后台核对。",
      );
      setRepricing(null);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function readCurrent(product: Goods, sizeID: number) {
    setBusy(true);
    setMessage("");
    try {
      const current = await api<Goods>("read", { nmID: product.nmID });
      setGoods(rows => rows.map(row => row.nmID === current.nmID ? current : row));
      const size = current.sizes.find(row => row.sizeID === sizeID);
      if (!size) {
        setMessage("当前规格已变更，请在更新后的列表中重新选择。");
        return;
      }
      setRepricing({ goods: current, sizeID });
      setField("price");
      setNewPrice(String(size.price));
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function status() {
    setBusy(true);
    try {
      const r = await api("status", { uploadID: task });
      setMessage(JSON.stringify(r));
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  function calculate(g: Goods, price: number) {
    const c = costs[`${mode}:${g.nmID}`];
    if (!c || g.currencyIsoCode4217 !== "RUB") return null;
    const keys =
      mode === "overseas"
        ? ["purchase", "first", "storage", "last", "other"]
        : ["purchase", "domestic", "international", "customs", "other"];
    if (
      Object.values(c).some((v) => v === "") ||
      Object.entries(c).some(
        ([k, v]) =>
          k !== "mode" && (!Number.isFinite(Number(v)) || Number(v) < 0),
      )
    )
      return null;
    const exchange = Number(c.exchange),
      rate =
        (Number(c.commission) + Number(c.advertising) + Number(c.loss)) / 100,
      target = Number(c.target) / 100;
    if (exchange <= 0 || rate >= 1 || target >= 1 || rate + target >= 1)
      return null;
    const cost = keys.reduce((sum, k) => sum + Number(c[k as keyof Cost]), 0),
      revenue = price / exchange,
      profit = revenue * (1 - rate) - cost;
    return {
      profit,
      margin: revenue > 0 ? profit / revenue : 0,
      breakeven: (cost * exchange) / (1 - rate),
      target: (cost * exchange) / (1 - rate - target),
    };
  }
  return (
    <section className="wbo-panel">
      <h2>价格与利润监控</h2>
      <div className="wbo-actions">
        <select
          value={mode}
          onChange={(e) => {
            setMode(e.target.value);
            setEdit(null);
          }}
        >
          <option value="overseas">WB 跨境海外仓</option>
          <option value="direct">WB 跨境国内直发</option>
        </select>
        <input
          placeholder="搜索 WB 商品编号、货号"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button disabled={busy || !ready} onClick={() => void sync()}>
          读取全店当前价格
        </button>
      </div>
      {task && (
        <button disabled={busy} onClick={() => void status()}>
          查询改价结果 #{task}
        </button>
      )}
      <p>
        成本按店铺、商品和配送模式分别保存。费用填写单件人民币金额，利润按卖家折后价扣除佣金、广告和损耗预留计算。
        {updated && ` 全店价格上次同步于 ${new Date(updated).toLocaleString("zh-CN")}`}
      </p>
      {message && <p role="status">{message}</p>}
      <div className="wbo-table-wrap">
        <table>
          <thead>
            <tr>
              <th>商品 / 货号</th>
              <th>规格</th>
              <th>原价 / 折后价</th>
              <th>普通折扣</th>
              <th>WB Club 折扣 / 会员价</th>
              <th>单件利润 CNY</th>
              <th>利润率</th>
              <th>保本价 RUB</th>
              <th>目标售价 RUB</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {goods
              .filter((g) =>
                `${g.nmID} ${g.vendorCode}`
                  .toLowerCase()
                  .includes(query.toLowerCase()),
              )
              .flatMap((g) =>
                g.sizes.map((s) => {
                  const result = calculate(g, s.discountedPrice);
                  return (
                    <tr key={`${g.nmID}:${s.sizeID}`}>
                      <td>
                        {g.nmID}
                        <small>{g.vendorCode}</small>
                      </td>
                      <td>{s.techSizeName}</td>
                      <td>
                        {s.price} / {s.discountedPrice} {g.currencyIsoCode4217}
                      </td>
                      <td>{g.discount == null ? "—" : `${g.discount}%`}</td>
                      <td>
                        {g.clubDiscount == null ? "—" : `${g.clubDiscount}%`}
                        {" / "}
                        {s.clubDiscountedPrice == null ? "—" : `${s.clubDiscountedPrice} ${g.currencyIsoCode4217}`}
                      </td>
                      <td>{result?.profit.toFixed(2) ?? "待补成本或汇率"}</td>
                      <td>
                        {result ? `${(result.margin * 100).toFixed(1)}%` : "—"}
                      </td>
                      <td>{result?.breakeven.toFixed(2) ?? "—"}</td>
                      <td>{result?.target.toFixed(2) ?? "—"}</td>
                      <td>
                        <button
                          onClick={() => {
                            setEdit(g.nmID);
                            setDraft(
                              costs[`${mode}:${g.nmID}`] ?? {
                                ...defaults,
                                mode,
                              },
                            );
                          }}
                        >
                          设置成本
                        </button>
                        <button
                          disabled={busy}
                          onClick={() => void readCurrent(g, s.sizeID)}
                        >
                          读取现价 / 编辑
                        </button>
                      </td>
                    </tr>
                  );
                }),
              )}
          </tbody>
        </table>
      </div>
      {!goods.length && <p>同步商品后开始核价，Token 需具备价格与折扣权限。</p>}
      {repricing && (() => {
        const product = repricing.goods;
        const size = product.sizes.find(row => row.sizeID === repricing.sizeID)!;
        const current = field === "price" ? size.price : product[field];
        const estimated = field === "clubDiscount"
          ? size.discountedPrice * (1 - Number(newPrice) / 100)
          : (field === "price" ? Number(newPrice) : size.price)
            * (1 - Number(field === "discount" ? newPrice : product.discount) / 100);
        return (
        <div className="wbo-overlay">
          <form
            className="wbo-dialog"
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <h2>{product.vendorCode || product.nmID} · 当前价格</h2>
            <p>WB 商品 {product.nmID} · 规格 {size.techSizeName || size.sizeID}</p>
            <p>原价 {size.price} · 普通折后价 {size.discountedPrice} · 会员价 {size.clubDiscountedPrice ?? "—"} {product.currencyIsoCode4217}</p>
            <p>普通折扣 {product.discount == null ? "—" : `${product.discount}%`} · WB Club 折扣 {product.clubDiscount == null ? "—" : `${product.clubDiscount}%`}</p>
            <label>
              修改项目
              <select value={field} disabled={busy} onChange={e => {
                const nextField = e.target.value as PriceField;
                const value = nextField === "price" ? size.price : product[nextField];
                setField(nextField);
                setNewPrice(value == null ? "" : String(value));
                setMessage("");
              }}>
                {Object.entries(priceFields).map(([key, label]) => <option key={key} value={key}>{label}</option>)}
              </select>
            </label>
            <label>
              新{priceFields[field]}（{field === "price" ? product.currencyIsoCode4217 : "%"}）
              <input
                required
                type="number"
                disabled={busy}
                min={field === "price" ? 1 : 0}
                max={field === "price" ? 100000000 : 99}
                step="1"
                value={newPrice}
                onChange={(e) => setNewPrice(e.target.value)}
              />
            </label>
            <p>生效范围 · {field === "price" && product.editableSizePrice ? "当前规格" : "该商品全部规格"}</p>
            {newPrice !== "" && Number.isFinite(estimated) && (
              <p>当前规格预计{field === "clubDiscount" ? "会员价" : "普通折后价"} {estimated.toFixed(2)} {product.currencyIsoCode4217}</p>
            )}
            {message && <p role="status">{message}</p>}
            <button disabled={busy || newPrice === "" || Number(newPrice) === current}>确认提交至 WB</button>
            <button
              type="button"
              disabled={busy}
              onClick={() => setRepricing(null)}
            >
              取消
            </button>
          </form>
        </div>
        );
      })()}
      {edit != null && (
        <div className="wbo-overlay">
          <form
            className="wbo-dialog"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <h2>
              商品 {edit} · {mode === "overseas" ? "海外仓" : "国内直发"}
            </h2>
            {Object.entries(labels)
              .filter(
                ([k]) =>
                  !(
                    mode === "overseas"
                      ? ["domestic", "international", "customs"]
                      : ["first", "storage", "last"]
                  ).includes(k),
              )
              .map(([key, label]) => (
                <label key={key}>
                  {label}
                  <input
                    required
                    type="number"
                    min={key === "exchange" ? "0.000001" : "0"}
                    step="any"
                    value={draft[key as keyof Cost]}
                    onChange={(e) =>
                      setDraft({ ...draft, [key]: e.target.value })
                    }
                  />
                </label>
              ))}
            <button disabled={busy}>保存成本</button>
            <button type="button" onClick={() => setEdit(null)}>
              关闭
            </button>
          </form>
        </div>
      )}
    </section>
  );
}
