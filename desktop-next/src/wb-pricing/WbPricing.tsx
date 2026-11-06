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
  sizes: {
    sizeID: number;
    price: number;
    discountedPrice: number;
    techSizeName: string;
  }[];
};
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
  const [repricing, setRepricing] = useState<Goods | null>(null),
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
    if (!repricing) return;
    setBusy(true);
    try {
      const r = await api<{ data?: { id?: number } }>("submit", {
        nmID: repricing.nmID,
        price: Number(newPrice),
        discount: repricing.discount,
      });
      setTask(r.data?.id ?? null);
      setMessage(
        `改价已提交，等待 WB 处理。任务 ${r.data?.id ?? "请在后台核对"}`,
      );
      setRepricing(null);
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
          同步商品价格
        </button>
      </div>
      {task && (
        <button disabled={busy} onClick={() => void status()}>
          查询改价结果 #{task}
        </button>
      )}
      <p>
        成本按店铺、商品和配送模式分别保存。费用填写单件人民币金额，利润按卖家折后价扣除佣金、广告和损耗预留计算。
        {updated && `同步时间 ${new Date(updated).toLocaleString("zh-CN")}`}
      </p>
      {message && <p role="status">{message}</p>}
      <div className="wbo-table-wrap">
        <table>
          <thead>
            <tr>
              <th>商品 / 货号</th>
              <th>规格</th>
              <th>原价 / 折后价</th>
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
                          disabled={busy || g.currencyIsoCode4217 !== "RUB"}
                          onClick={() => {
                            setRepricing(g);
                            setNewPrice(String(s.price));
                          }}
                        >
                          改价
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
      {repricing && (
        <div className="wbo-overlay">
          <form
            className="wbo-dialog"
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <h2>确认修改商品 {repricing.nmID}</h2>
            <p>
              此操作向 WB 提交该商品全部规格的统一原价，折扣为{" "}
              {repricing.discount}%。
            </p>
            <label>
              新原价 RUB
              <input
                required
                type="number"
                min="1"
                max="100000000"
                step="1"
                value={newPrice}
                onChange={(e) => setNewPrice(e.target.value)}
              />
            </label>
            <p>
              新折后价{" "}
              {(Number(newPrice) * (1 - repricing.discount / 100)).toFixed(2)}{" "}
              RUB
            </p>
            <p>
              测算单件利润{" "}
              {calculate(
                repricing,
                Number(newPrice) * (1 - repricing.discount / 100),
              )?.profit.toFixed(2) ?? "成本未完整设置"}{" "}
              CNY
            </p>
            <button disabled={busy}>确认提交至 WB</button>
            <button
              type="button"
              disabled={busy}
              onClick={() => setRepricing(null)}
            >
              取消
            </button>
          </form>
        </div>
      )}
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
