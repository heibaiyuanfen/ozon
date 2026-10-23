import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./warehouse-switch.css";
const validSku = (s: string) =>
  /^[1-9]\d*$/.test(s) && Number.isSafeInteger(Number(s));
type Product = { sku: string; offerId: string; name: string };
type Warehouse = { id: number; name: string; kind: string; status: string };
type Stock = {
  sku: string;
  warehouseId: number;
  name: string;
  free: number | null;
  reserved: number | null;
};
type Edit = { quantity: string; sourceId: string; sourceQuantity: string };
type Row = Product & {
  source: number;
  sourceId: number;
  sourceQuantity: number;
  target: number;
  quantity: number;
  status: string;
  actualSource?: number | null;
  actualTarget?: number | null;
  observedAt?: string;
};
type Plan = { id: string; targetId: number; rows: Row[]; status: string };
const valid = (s: string) =>
  s.trim() !== "" &&
  Number.isSafeInteger(Number(s)) &&
  Number(s) >= 0 &&
  Number(s) <= 1000000;
export function WarehouseSwitch({ shopId }: { shopId: string }) {
  const [options, setOptions] = useState<{
    warehouses: Warehouse[];
    products: Product[];
  }>({ warehouses: [], products: [] });
  const [stock, setStock] = useState<Record<string, Stock[]>>({});
  const [target, setTarget] = useState("");
  const [query, setQuery] = useState("");
  const [edits, setEdits] = useState<Record<string, Edit>>({});
  const [plan, setPlan] = useState<Plan | null>(null);
  const [history, setHistory] = useState<Plan[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [page, setPage] = useState(0);
  const [targetBulk, setTargetBulk] = useState("");
  const [sourceBulk, setSourceBulk] = useState("0");
  const [executing, setExecuting] = useState(false);
  const [excludeTargetStock, setExcludeTargetStock] = useState(false);
  const [resultFilter, setResultFilter] = useState("全部");
  const [pollError, setPollError] = useState("");
  const finishedId = useRef("");
  const category = (r: Row) =>
    r.status.startsWith("成功")
      ? "成功"
      : r.status.startsWith("已跳过")
        ? "跳过"
        : r.status.startsWith("未修改")
          ? "失败"
          : r.status.startsWith("部分完成")
            ? "部分完成"
            : r.status.startsWith("目标仓未确认")
              ? "待核对"
              : r.status === "待提交"
                ? "待处理"
                : "处理中";
  const receive = (next: Plan) => {
    if (finishedId.current === next.id && next.status !== "finished") return;
    if (next.status === "finished") finishedId.current = next.id;
    setPlan((current) =>
      current?.status === "finished" && next.status !== "finished"
        ? current
        : next,
    );
    setStock((old) => {
      const copy = { ...old };
      for (const r of next.rows) {
        const rows = [...(copy[r.sku] ?? [])];
        for (const [wid, value] of [
          [r.sourceId, r.actualSource],
          [next.targetId, r.actualTarget],
        ] as [number, number | null | undefined][]) {
          if (value == null) continue;
          const index = rows.findIndex((s) => s.warehouseId === wid);
          if (index >= 0) rows[index] = { ...rows[index], free: value };
          else
            rows.push({
              sku: r.sku,
              warehouseId: wid,
              name: "",
              free: value,
              reserved: null,
            });
        }
        copy[r.sku] = rows;
      }
      return copy;
    });
  };
  useEffect(() => {
    if (!executing || !plan?.id) return;
    let active = true,
      inFlight = false;
    const planId = plan.id;
    const poll = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const next = await invoke<Plan>("warehouse_switch_progress", {
          planId,
          expectedShopId: shopId,
        });
        if (active) {
          if (next.status !== "preview") receive(next);
          setPollError("");
        }
      } catch (e) {
        if (active)
          setPollError(
            `进度刷新失败：${String(e)}；不代表库存修改失败，请勿重复提交。`,
          );
      } finally {
        inFlight = false;
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), 1500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [executing, plan?.id, shopId]);
  useEffect(() => {
    let active = true;
    invoke<Plan[]>("warehouse_switch_history")
      .then((v) => {
        if (active) setHistory(v);
      })
      .catch((e) => {
        if (active) setMessage(String(e));
      });
    return () => {
      active = false;
    };
  }, [shopId]);
  const warehouse = (id: number) => options.warehouses.find((w) => w.id === id);
  const editable = (w: Warehouse | undefined) =>
    !!w && w.status === "created" && ["rfbs", "fbs", "ogl"].includes(w.kind);
  const candidates = (sku: string) =>
    (stock[sku] ?? []).filter(
      (s) =>
        s.free != null &&
        String(s.warehouseId) !== target &&
        editable(warehouse(s.warehouseId)),
    );
  const autoSource = (sku: string) => {
    const all = candidates(sku);
    const positive = all.filter((s) => (s.free ?? 0) > 0);
    return positive.length === 1
      ? String(positive[0].warehouseId)
      : all.length === 1
        ? String(all[0].warehouseId)
        : "";
  };
  const initial = (p: Product): Edit => ({
    quantity: targetBulk,
    sourceId: autoSource(p.sku),
    sourceQuantity: sourceBulk,
  });
  const targetHasStock = (sku: string, wid = Number(target)) =>
    (stock[sku] ?? []).some(
      (s) =>
        s.warehouseId === wid && ((s.free ?? 0) > 0 || (s.reserved ?? 0) > 0),
    );
  const removeExisting = (wid = Number(target)) => {
    setEdits((old) =>
      Object.fromEntries(
        Object.entries(old).filter(([sku]) => !targetHasStock(sku, wid)),
      ),
    );
    setPlan((p) => (p?.status === "preview" ? null : p));
  };
  const filtered = options.products.filter(
    (p) =>
      `${p.offerId} ${p.sku} ${p.name}`
        .toLowerCase()
        .includes(query.toLowerCase()) &&
      (!excludeTargetStock || !targetHasStock(p.sku)),
  );
  const eligible = filtered.filter((p) => candidates(p.sku).length > 0);
  const chosen = Object.keys(edits).length;
  const allChecked =
    eligible.length > 0 && eligible.every((p) => p.sku in edits);
  const setEdit = (sku: string, key: keyof Edit, value: string) => {
    setEdits((old) => ({ ...old, [sku]: { ...old[sku], [key]: value } }));
    setPlan(null);
  };
  const select = (rows: Product[], on: boolean) => {
    if (
      on &&
      new Set([...Object.keys(edits), ...rows.map((r) => r.sku)]).size > 5000
    ) {
      setMessage("单批最多5000件，请缩小筛选范围后全选。");
      return;
    }
    setEdits((old) => {
      const next = { ...old };
      for (const p of rows) {
        if (on) next[p.sku] = next[p.sku] ?? initial(p);
        else delete next[p.sku];
      }
      return next;
    });
    setPlan(null);
  };
  const load = async () => {
    setBusy(true);
    setPlan(null);
    setEdits({});
    setStock({});
    setPage(0);
    try {
      const data = await invoke<typeof options>("warehouse_switch_options");
      setOptions(data);
      const readable = data.products.filter((p) => validSku(p.sku));
      let failed = data.products.length - readable.length;
      for (let n = 0; n < readable.length; n += 100) {
        setMessage(
          `正在自动读取原仓与库存：${n}/${readable.length}；已跳过 ${failed} 件异常商品`,
        );
        const products = readable.slice(n, n + 100);
        const read = (rows: Product[]) =>
          invoke<{ stocks: Stock[] }>("warehouse_switch_stocks", {
            skus: rows.map((p) => p.sku),
            expectedShopId: shopId,
          });
        let result: { stocks: Stock[] };
        try {
          result = await read(products);
        } catch (error) {
          if (!String(error).includes("400")) throw error;
          result = { stocks: [] };
          for (const product of products) {
            try {
              result.stocks.push(...(await read([product])).stocks);
            } catch (e) {
              if (!String(e).includes("400")) throw e;
              failed++;
            }
          }
        }
        setStock((old) => {
          const next = { ...old };
          for (const p of products)
            next[p.sku] = result.stocks.filter((s) => s.sku === p.sku);
          return next;
        });
      }
      setMessage(
        `库存读取完成；${failed} 件SKU无效或平台拒绝读取，已跳过。已读取且可修改的商品可继续预览、上传。FBP仅展示。`,
      );
    } catch (e) {
      setMessage(
        `读取未全部完成：${String(e)}；未读取商品不可提交，可重新读取。`,
      );
    } finally {
      setBusy(false);
    }
  };
  const apply = () => {
    if (!valid(targetBulk) || !valid(sourceBulk)) {
      setMessage("批量库存须填写0至1000000整数");
      return;
    }
    setEdits((old) =>
      Object.fromEntries(
        Object.entries(old).map(([k, v]) => [
          k,
          { ...v, quantity: targetBulk, sourceQuantity: sourceBulk },
        ]),
      ),
    );
    setPlan(null);
    setMessage(`已将填写值应用到 ${chosen} 件选中商品，尚未提交平台。`);
  };
  const preview = async () => {
    setBusy(true);
    setPlan(null);
    setMessage("");
    try {
      const items = Object.entries(edits).map(([sku, e]) => {
        if (!valid(e.quantity) || !valid(e.sourceQuantity) || !e.sourceId)
          throw Error(`${sku} 请填写两仓库存；多个原仓时请先选择原仓`);
        if (e.sourceId === target) throw Error(`${sku} 原仓与目标仓相同`);
        return {
          sku,
          source_id: Number(e.sourceId),
          source_quantity: Number(e.sourceQuantity),
          quantity: Number(e.quantity),
        };
      });
      setPlan(
        await invoke("warehouse_switch_preview", {
          targetId: Number(target),
          items,
          expectedShopId: shopId,
          excludeTargetStock,
        }),
      );
      setMessage(
        "预览已生成，请核对两仓数量变化，再点击顶部“确认上传库存修改”。目前尚未写入平台。",
      );
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  const execute = async () => {
    if (
      !plan ||
      !window.confirm(
        `确认覆盖 ${plan.rows.length} 件商品的库存？目标仓设为预览数量；确认后将每件商品所选原仓设为预览数量。其他仓及FBP不修改。已有订单、预留库存和实物不迁移；两步可能部分完成。`,
      )
    )
      return;
    finishedId.current = "";
    setBusy(true);
    setExecuting(true);
    setPollError("");
    setResultFilter("全部");
    try {
      receive(
        await invoke<Plan>("warehouse_switch_execute", {
          planId: plan.id,
          expectedShopId: shopId,
        }),
      );
      setMessage(
        "执行结束，请逐项核对结果；不明确或部分完成时请先核对平台，不要直接重试。 ",
      );
    } catch (e) {
      setPlan((p) => (p ? { ...p, status: "unknown" } : p));
      setMessage(`结果需核对：${String(e)}；请读取历史记录。`);
    } finally {
      setExecuting(false);
      try {
        setHistory(await invoke("warehouse_switch_history"));
      } catch {}
      setBusy(false);
    }
  };
  const name = (id: number) => warehouse(id)?.name ?? String(id);
  return (
    <section className="card table-card warehouse-switch">
      <header>
        <h2>跨境商品批量切仓与库存</h2>
        <p>
          自动读取原仓与可售库存，选定目标仓和覆盖数量，再设置原仓保留数量。FBP及未识别类型仓库仅显示，不修改。
        </p>
        <button className="primary" disabled={busy} onClick={() => void load()}>
          {busy ? "读取 / 处理中…" : "读取仓库、商品及实时库存"}
        </button>
      </header>
      <div className="ws-controls">
        <label>
          目标仓库
          <select
            disabled={busy}
            value={target}
            onChange={(e) => {
              setTarget(e.target.value);
              if (excludeTargetStock) removeExisting(Number(e.target.value));
              setEdits((old) =>
                Object.fromEntries(
                  Object.entries(old).map(([sku, v]) => [
                    sku,
                    v.sourceId === e.target.value ? { ...v, sourceId: "" } : v,
                  ]),
                ),
              );
              setPlan(null);
            }}
          >
            <option value="">请选择目标仓库</option>
            {options.warehouses.filter(editable).map((w) => (
              <option key={w.id} value={String(w.id)}>
                {w.name} · {w.kind}
              </option>
            ))}
          </select>
        </label>
        <label>
          目标仓覆盖库存
          <input
            type="number"
            min={0}
            max={1000000}
            disabled={busy}
            value={targetBulk}
            placeholder="批量填写"
            onChange={(e) => setTargetBulk(e.target.value)}
          />
        </label>
        <label>
          原仓调整后库存
          <input
            type="number"
            min={0}
            max={1000000}
            disabled={busy}
            value={sourceBulk}
            onChange={(e) => setSourceBulk(e.target.value)}
          />
        </label>
        <button disabled={busy || !chosen} onClick={apply}>
          应用到已选商品
        </button>
      </div>
      <div className="ws-tools">
        <input
          placeholder="搜索货号、SKU、名称"
          aria-label="切仓商品搜索"
          value={query}
          disabled={busy}
          onChange={(e) => {
            setQuery(e.target.value);
            setPage(0);
          }}
        />
        <button
          disabled={busy || !eligible.length}
          onClick={() => select(eligible, true)}
        >
          全选筛选结果（{eligible.length}）
        </button>
        <button
          disabled={busy || !chosen}
          onClick={() => {
            setEdits({});
            setPlan(null);
          }}
        >
          取消全部选择
        </button>
        <span>
          已选 {chosen} 件 · 筛选 {filtered.length} 件
        </span>
      </div>
      <div className="ws-submit-bar">
        <div>
          <strong>库存修改提交</strong>
          <small>
            填写数量、应用到已选商品后，先生成预览，再确认上传到 Ozon。
          </small>
        </div>
        <button
          className="primary"
          disabled={busy || !chosen || !target}
          onClick={() => void preview()}
        >
          ① 核对修改并生成预览
        </button>
        <button
          className="primary"
          disabled={busy || plan?.status !== "preview"}
          onClick={() => void execute()}
        >
          ② 确认上传库存修改
          {plan?.status === "preview" ? `（${plan.rows.length}件）` : ""}
        </button>
        {!plan && <small>尚未生成预览，上传按钮暂不可用。</small>}
      </div>
      <div className="ws-tools">
        <label>
          <input
            type="checkbox"
            disabled={busy || !target}
            checked={excludeTargetStock}
            onChange={(e) => {
              setExcludeTargetStock(e.target.checked);
              if (e.target.checked) removeExisting();
              setPlan((p) => (p?.status === "preview" ? null : p));
              setPage(0);
            }}
          />{" "}
          排除目标仓已有库存（含预留）
        </label>
        <button disabled={busy || !target} onClick={() => removeExisting()}>
          从已选中去除目标仓已有库存
        </button>
        <span>
          目标仓已有库存：
          {options.products.filter((p) => targetHasStock(p.sku)).length}件
        </span>
      </div>
      {plan && (
        <div className="ws-progress" role="status">
          <strong>{executing ? "正在提交库存修改" : "本批执行状态"}</strong>
          <p>
            共{plan.rows.length}件 ·{" "}
            {["成功", "失败", "部分完成", "待核对", "跳过", "处理中", "待处理"]
              .map(
                (k) =>
                  `${k} ${plan.rows.filter((r) => category(r) === k).length}`,
              )
              .join(" · ")}
          </p>
          <progress
            max={plan.rows.length || 1}
            value={
              plan.rows.filter(
                (r) => !["处理中", "待处理"].includes(category(r)),
              ).length
            }
          />
          <p>
            每1.5秒读取执行记录；“最近回读”是平台已返回的数量，不把填写的目标值当作实际库存。
          </p>
          {pollError && <p>{pollError}</p>}
        </div>
      )}
      {message && (
        <p role="status" className="sync-message">
          {message}
        </p>
      )}
      <div className="ws-table">
        <table>
          <thead>
            <tr>
              <th>
                <input
                  type="checkbox"
                  aria-label="全选筛选结果"
                  disabled={busy || !eligible.length}
                  checked={allChecked}
                  onChange={(e) => select(eligible, e.target.checked)}
                />
              </th>
              <th>商品</th>
              <th>自动读取：所在仓库 / 可售 / 预留</th>
              <th>要修改的原仓</th>
              <th>原仓调整后</th>
              <th>目标仓覆盖库存</th>
              <th>执行结果 / 最近回读</th>
            </tr>
          </thead>
          <tbody>
            {filtered.slice(page * 30, page * 30 + 30).map((p) => {
              const e = edits[p.sku];
              const c = candidates(p.sku);
              const result = plan?.rows.find((r) => r.sku === p.sku);
              return (
                <tr key={p.sku}>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`选择${p.offerId}`}
                      disabled={busy || !c.length}
                      checked={!!e}
                      onChange={(ev) => select([p], ev.target.checked)}
                    />
                  </td>
                  <td>
                    <b>{p.offerId}</b>
                    <small>
                      SKU {p.sku} · {p.name}
                    </small>
                  </td>
                  <td>
                    {!(p.sku in stock)
                      ? "尚未读取"
                      : !stock[p.sku].length
                        ? "平台未返回库存记录"
                        : stock[p.sku].map((s) => (
                            <small key={s.warehouseId}>
                              {s.name || name(s.warehouseId)} ·{" "}
                              {s.free ?? "未知"} / {s.reserved ?? "未知"}
                              {warehouse(s.warehouseId)?.kind === "fbp"
                                ? " · FBP保持不变"
                                : !editable(warehouse(s.warehouseId))
                                  ? " · 不可修改"
                                  : ""}
                            </small>
                          ))}
                  </td>
                  <td>
                    {e ? (
                      <select
                        aria-label={`${p.offerId}原仓`}
                        disabled={busy}
                        value={e.sourceId}
                        onChange={(ev) =>
                          setEdit(p.sku, "sourceId", ev.target.value)
                        }
                      >
                        <option value="">多个原仓，请选择</option>
                        {c.map((s) => (
                          <option
                            key={s.warehouseId}
                            value={String(s.warehouseId)}
                          >
                            {s.name || name(s.warehouseId)} · {s.free}
                          </option>
                        ))}
                      </select>
                    ) : c.length === 1 ? (
                      c[0].name
                    ) : c.length > 1 ? (
                      "多个原仓，可选择"
                    ) : (
                      "无可修改原仓"
                    )}
                  </td>
                  <td>
                    <input
                      type="number"
                      min={0}
                      max={1000000}
                      aria-label={`${p.offerId}原仓调整后库存`}
                      disabled={busy || !e}
                      value={e?.sourceQuantity ?? ""}
                      onChange={(ev) =>
                        setEdit(p.sku, "sourceQuantity", ev.target.value)
                      }
                    />
                  </td>
                  <td>
                    <input
                      type="number"
                      min={0}
                      max={1000000}
                      aria-label={`${p.offerId}目标库存`}
                      disabled={busy || !e}
                      placeholder="请填写"
                      value={e?.quantity ?? ""}
                      onChange={(ev) =>
                        setEdit(p.sku, "quantity", ev.target.value)
                      }
                    />
                  </td>
                  <td>
                    {result ? (
                      <>
                        <strong>{category(result)}</strong>
                        <small>{result.status}</small>
                        <small>
                          原仓 {result.actualSource ?? "未知"} / 目标{" "}
                          {result.actualTarget ?? "未知"}
                        </small>
                        <small>
                          {result.observedAt
                            ? new Date(result.observedAt).toLocaleTimeString(
                                "zh-CN",
                              )
                            : "尚未回读"}
                        </small>
                      </>
                    ) : targetHasStock(p.sku) ? (
                      "目标仓已有库存"
                    ) : (
                      "尚未提交"
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div className="ws-tools">
        <button disabled={busy || !page} onClick={() => setPage((p) => p - 1)}>
          上一页
        </button>
        <span>
          {page + 1}/{Math.max(1, Math.ceil(filtered.length / 30))}
        </span>
        <button
          disabled={busy || (page + 1) * 30 >= filtered.length}
          onClick={() => setPage((p) => p + 1)}
        >
          下一页
        </button>
        <button
          className="primary"
          disabled={busy || !chosen || !target}
          onClick={() => void preview()}
        >
          核对实时库存并预览
        </button>
      </div>
      {plan && (
        <>
          <h3>本批预览 / 实时执行结果</h3>
          <label>
            筛选结果{" "}
            <select
              value={resultFilter}
              onChange={(e) => setResultFilter(e.target.value)}
            >
              {[
                "全部",
                "成功",
                "失败",
                "部分完成",
                "待核对",
                "跳过",
                "处理中",
                "待处理",
              ].map((v) => (
                <option key={v}>{v}</option>
              ))}
            </select>
          </label>
          <p>
            目标仓：{name(plan.targetId)}
            ；仅修改表中所选原仓，其他仓及FBP保持原值。
          </p>
          <div className="ws-table">
            <table>
              <thead>
                <tr>
                  <th>货号</th>
                  <th>原仓与库存变化</th>
                  <th>目标库存变化</th>
                  <th>结果</th>
                  <th>最近回读库存：原仓 / 目标仓</th>
                </tr>
              </thead>
              <tbody>
                {plan.rows
                  .filter(
                    (r) =>
                      resultFilter === "全部" || category(r) === resultFilter,
                  )
                  .map((r) => (
                    <tr key={r.sku}>
                      <td>{r.offerId}</td>
                      <td>
                        {name(r.sourceId)}：{r.source} → {r.sourceQuantity}
                      </td>
                      <td>
                        {r.target} → {r.quantity}
                      </td>
                      <td>{r.status}</td>
                      <td>
                        {r.actualSource ?? "未知"} / {r.actualTarget ?? "未知"}
                        <small>
                          {r.observedAt
                            ? new Date(r.observedAt).toLocaleTimeString("zh-CN")
                            : "尚未回读"}
                        </small>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          <button
            className="primary"
            disabled={busy || plan.status !== "preview"}
            onClick={() => void execute()}
          >
            确认批量修改仓库库存
          </button>
        </>
      )}
      <details>
        <summary>最近20批操作记录</summary>
        <button
          disabled={busy}
          onClick={() =>
            void invoke<Plan[]>("warehouse_switch_history")
              .then(setHistory)
              .catch((e) => setMessage(String(e)))
          }
        >
          读取历史记录
        </button>
        {history.map((p) => (
          <details key={p.id}>
            <summary>
              {p.id} · {p.status} · {p.rows.length}件
            </summary>
            <p>目标仓 {name(p.targetId)}</p>
            <button disabled={busy} onClick={()=>{setPlan(p);setResultFilter("全部");setMessage(p.status==="running"?"此历史批次没有结束标记，可能已中断，请先核对平台，不要重复提交。":"已打开历史逐项结果，可按成功、待核对等状态筛选。");}}>查看逐项结果</button>
            {p.rows.map((r) => (
              <p key={r.sku}>
                {r.offerId}：原仓 {r.sourceQuantity ?? 0}，目标 {r.quantity} ·{" "}
                {r.status}
              </p>
            ))}
          </details>
        ))}
      </details>
    </section>
  );
}
