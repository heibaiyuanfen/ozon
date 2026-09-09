import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { wbShopCenter } from "./bridge";
import "./wb-shop-api-center.css";
import "./product-master.css";
type Row = Record<string, any>;
type List = { rows: Row[]; spus: Row[]; total: number };
const call = <T,>(action: string, payload: Row = {}) =>
  invoke<T>("product_master", { action, payload });
const labels: Record<string, string> = {
  newSpuCode: "新建系列编码（可选）",
  newSpuName: "新建系列名称",
  code: "内部编码",
  name: "中文名称",
  nameRu: "俄文名称",
  spuId: "所属系列",
  brand: "内部品牌",
  color: "颜色",
  size: "规格 / 尺寸",
  category: "ERP 类目",
  supplier: "供应商编码",
  purchaseCost: "采购成本",
  purchaseCurrency: "采购币种",
  widthCm: "宽 cm",
  lengthCm: "长 cm",
  heightCm: "高 cm",
  weightKg: "重量 kg",
  packSize: "包装数量",
  status: "状态",
  reason: "修改 / 冲突解决原因",
  attributesText: "扩展属性",
  mediaText: "商品图片",
};
const empty: Row = {
  newSpuCode: "",
  newSpuName: "",
  code: "",
  name: "",
  nameRu: "",
  spuId: "",
  brand: "",
  color: "",
  size: "",
  category: "",
  supplier: "",
  purchaseCost: "",
  purchaseCurrency: "CNY",
  widthCm: "",
  lengthCm: "",
  heightCm: "",
  weightKg: "",
  packSize: "",
  status: "active",
  attributesText: "{}",
  mediaText: "[]",
  reason: "",
};
function download(name: string, text: string) {
  const url = URL.createObjectURL(
    new Blob(["\ufeff" + text], { type: "text/csv;charset=utf-8" }),
  );
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
const csv = (rows: Row[]) => {
  const keys = Object.keys(rows[0] || {});
  return [keys, ...rows.map((r) => keys.map((k) => r[k] ?? ""))]
    .map((r) =>
      r.map((v) => '"' + String(v).replaceAll('"', '""') + '"').join(","),
    )
    .join("\r\n");
};
function PropertyEditor({
  value,
  onChange,
  media = false,
}: {
  value: string;
  onChange: (v: string) => void;
  media?: boolean;
}) {
  const parsed = JSON.parse(value || (media ? "[]" : "{}"));
  if (media) {
    const rows = parsed as Row[];
    return (
      <div>
        {rows.map((r, i) => (
          <div className="pm-property" key={i}>
            <input
              aria-label="图片地址"
              placeholder="https:// 图片地址"
              value={r.url || ""}
              onChange={(e) =>
                onChange(
                  JSON.stringify(
                    rows.map((x, j) =>
                      j === i ? { ...x, url: e.target.value } : x,
                    ),
                  ),
                )
              }
            />
            <select
              aria-label="图片来源"
              value={r.source || "ERP"}
              onChange={(e) =>
                onChange(
                  JSON.stringify(
                    rows.map((x, j) =>
                      j === i ? { ...x, source: e.target.value } : x,
                    ),
                  ),
                )
              }
            >
              {["ERP", "WILDBERRIES", "SUPPLIER"].map((s) => (
                <option key={s}>{s}</option>
              ))}
            </select>
            <button
              type="button"
              onClick={() =>
                onChange(
                  JSON.stringify(
                    rows.map((x, j) => ({ ...x, isPrimary: j === i })),
                  ),
                )
              }
            >
              {r.isPrimary ? "主图" : "设为主图"}
            </button>
            <button
              type="button"
              onClick={() =>
                onChange(JSON.stringify(rows.filter((_, j) => j !== i)))
              }
            >
              移除
            </button>
          </div>
        ))}
        <button
          type="button"
          onClick={() =>
            onChange(
              JSON.stringify([
                ...rows,
                { url: "", source: "ERP", isPrimary: rows.length === 0 },
              ]),
            )
          }
        >
          添加图片
        </button>
      </div>
    );
  }
  const rows = Object.entries(parsed);
  return (
    <div>
      {rows.map(([key, val], i) => (
        <div className="pm-property" key={i}>
          <input
            aria-label="属性名称"
            value={key}
            onChange={(e) => {
              const next = [...rows];
              next[i] = [e.target.value, val];
              onChange(JSON.stringify(Object.fromEntries(next)));
            }}
          />
          <input
            aria-label="属性值"
            value={
              typeof val === "object" ? JSON.stringify(val) : String(val ?? "")
            }
            readOnly={typeof val === "object"}
            onChange={(e) =>
              onChange(JSON.stringify({ ...parsed, [key]: e.target.value }))
            }
          />
          <button
            type="button"
            onClick={() =>
              onChange(
                JSON.stringify(
                  Object.fromEntries(rows.filter((_, j) => i !== j)),
                ),
              )
            }
          >
            移除
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() => {
          let i = rows.length + 1;
          while (parsed["属性" + i] !== undefined) i++;
          onChange(JSON.stringify({ ...parsed, ["属性" + i]: "" }));
        }}
      >
        添加属性
      </button>
    </div>
  );
}
export function ProductMasterPage() {
  const [tab, setTab] = useState("skus"),
    [query, setQuery] = useState(""),
    [page, setPage] = useState(0),
    [filters, setFilters] = useState<Row>({}),
    [data, setData] = useState<List>({ rows: [], spus: [], total: 0 }),
    [shops, setShops] = useState<Row[]>([]),
    [summary, setSummary] = useState<Row>({}),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [revision, setRevision] = useState(0),
    [syncMode, setSyncMode] = useState("incremental");
  const [form, setForm] = useState<Row | null>(null),
    [detail, setDetail] = useState<Row | null>(null),
    [detailTab, setDetailTab] = useState("概览"),
    [binding, setBinding] = useState<Row | null>(null),
    [skuQuery, setSkuQuery] = useState(""),
    [candidates, setCandidates] = useState<Row[]>([]),
    [preview, setPreview] = useState<Row | null>(null),
    [reason, setReason] = useState(""),
    [selected, setSelected] = useState<string[]>([]),
    [bulkField, setBulkField] = useState("status"),
    [bulkValue, setBulkValue] = useState("active"),
    [importKind, setImportKind] = useState("products"),
    [importResult, setImportResult] = useState<Row | null>(null);
  const sequence = useRef(0),
    kind = ["unmapped", "conflict"].includes(tab) ? "listings" : tab;
  const refresh = () => setRevision((x) => x + 1);
  useEffect(() => {
    void wbShopCenter<{ shops: Row[] }>("dashboard")
      .then((x) => setShops(x.shops))
      .catch((e) => setError(String(e)));
  }, []);
  useEffect(() => {
    let stopped = false;
    const update = () =>
      void call<Row>("summary", { shopId: filters.shopId || "" })
        .then((r) => {
          if (!stopped) setSummary(r);
        })
        .catch((e) => {
          if (!stopped) setError(String(e));
        });
    update();
    const timer = setInterval(update, 5000);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [filters.shopId, revision]);
  useEffect(() => {
    const id = ++sequence.current;
    if (tab === "tools") return;
    const timer = setTimeout(() => {
      setBusy(true);
      void call<List>("list_v2", {
        kind,
        query,
        page,
        ...filters,
        status:
          tab === "unmapped"
            ? "UNMAPPED"
            : tab === "conflict"
              ? "CONFLICT"
              : filters.status || "",
      })
        .then((r) => {
          if (id === sequence.current) setData(r);
        })
        .catch((e) => {
          if (id === sequence.current) setError(String(e));
        })
        .finally(() => {
          if (id === sequence.current) setBusy(false);
        });
    }, 200);
    return () => clearTimeout(timer);
  }, [tab, kind, query, page, filters, revision]);
  useEffect(() => {
    if (!binding) return;
    let active = true;
    const timer = setTimeout(
      () =>
        void call<List>(
          skuQuery ? "list_v2" : "suggest",
          skuQuery
            ? { kind: "skus", query: skuQuery }
            : { listingId: binding.id },
        )
          .then((r) => {
            if (active) setCandidates(r.rows);
          })
          .catch((e) => setError(String(e))),
      200,
    );
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [binding, skuQuery]);
  const run = async (action: string, p: Row, after?: () => void) => {
    setBusy(true);
    setError("");
    try {
      const result = await call<Row>(action, p);
      setNotice(
        action === "reprocess"
          ? `已处理 ${result.total} 条引用，${result.resolved} 条可解析`
          : "操作已保存",
      );
      after?.();
      refresh();
      return result;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setBusy(false);
    }
  };
  const show = async (r: Row, k = kind) => {
    try {
      setDetail({
        ...(await call<Row>("detail_v2", { id: r.id, kind: k })),
        kind: k,
      });
      setDetailTab("概览");
    } catch (e) {
      setError(String(e));
    }
  };
  const edit = (d: Row) => {
    const x = d.item;
    setDetail(null);
    setForm({
      ...empty,
      ...Object.fromEntries(
        Object.entries(x.metadata || {}).filter(([k]) => k in empty),
      ),
      id: x.id,
      action: d.kind === "spus" ? "update_spu" : "update_sku",
      code: x.code,
      name: x.name,
      status: x.status,
      spuId: x.spu_id || "",
      brand: x.brand || "",
      color: x.color || "",
      size: x.size || "",
      category: x.category || "",
      supplier: x.supplier || "",
      purchaseCost: x.purchase_cost || "",
      attributesText: x.attributes_json || "{}",
      mediaText: x.media_json || "[]",
    });
  };
  const createFrom = async (r: Row) => {
    try {
      const d = await call<Row>("detail_v2", { id: r.id, kind: "listings" });
      const x = JSON.parse(d.item.snapshot_json),
        c = x.card,
        z = x.size;
      setForm({
        ...empty,
        action: "create_sku",
        listingId: r.id,
        code: c.vendorCode || "",
        name: c.title || "",
        brand: c.brand || "",
        size: z.techSize || "",
        widthCm: String(c.dimensions?.width ?? ""),
        lengthCm: String(c.dimensions?.length ?? ""),
        heightCm: String(c.dimensions?.height ?? ""),
        weightKg: String(c.dimensions?.weightBrutto ?? ""),
        attributesText: JSON.stringify(
          { wbCharacteristics: c.characteristics || [] },
          null,
          2,
        ),
        mediaText: JSON.stringify(
          (c.photos || []).map((m: Row, i: number) => ({
            url: m.big || m.c516x688,
            source: "WILDBERRIES",
            isPrimary: i === 0,
          })),
          null,
          2,
        ),
      });
    } catch (e) {
      setError(String(e));
    }
  };
  const submit = async () => {
    if (!form) return;
    try {
      const payload = {
        ...form,
        attributes: JSON.parse(form.attributesText || "{}"),
        media: JSON.parse(form.mediaText || "[]"),
      };
      await run(form.action, payload, () => setForm(null));
    } catch (e) {
      setError("属性或媒体格式错误：" + String(e));
    }
  };
  const admin = summary.role === "admin",
    canEdit = admin || summary.role === "operator";
  const upload = async (file: File) => {
    if (file.size > 10 * 1024 * 1024) {
      setError("文件不能超过 10 MB");
      return;
    }
    setBusy(true);
    setImportResult(null);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      let binary = "";
      for (let i = 0; i < bytes.length; i += 8192)
        binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
      setImportResult(
        await call<Row>("import_preview", {
          kind: importKind,
          filename: file.name,
          base64: btoa(binary),
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const setFilter = (k: string, v: string) => {
    setFilters({ ...filters, [k]: v });
    setPage(0);
    setSelected([]);
  };
  return (
    <section className="wb-center pm-center">
      <header className="wb-center-head">
        <div>
          <span>WBERP · PRODUCT MASTER</span>
          <h1>商品资料中心</h1>
          <p>统一 SKU 身份 · 系列变体 · 多店铺映射</p>
        </div>
        <div>
          {admin && (
            <>
              <button
                onClick={() =>
                  setForm({
                    action: "create_spu",
                    code: "",
                    name: "",
                    status: "active",
                  })
                }
              >
                创建系列
              </button>
              <button
                className="primary"
                onClick={() => setForm({ ...empty, action: "create_sku" })}
              >
                创建 SKU
              </button>
            </>
          )}
          <select
            aria-label="同步模式"
            value={syncMode}
            onChange={(e) => setSyncMode(e.target.value)}
          >
            <option value="incremental">增量同步</option>
            <option value="full">全量核对</option>
          </select>
          <button
            disabled={busy || !admin}
            onClick={() => {
              if (!filters.shopId) {
                setError("请先选择要同步的店铺");
                return;
              }
              void wbShopCenter("sync", {
                shopId: filters.shopId,
                resourceType: "products",
                mode: syncMode,
              })
                .then(() => {
                  setNotice("同步已排队，下方状态每 5 秒更新");
                  refresh();
                })
                .catch((e) => setError(String(e)));
            }}
          >
            同步 WB 商品
          </button>
        </div>
      </header>
      {error && (
        <div className="wb-center-error" role="alert">
          {error}
          <button onClick={() => setError("")}>关闭</button>
        </div>
      )}
      {notice && (
        <div className="pm-notice" role="status">
          {notice}
        </div>
      )}
      <div className="pm-counts">
        {[
          ["MAPPED", "已匹配"],
          ["UNMAPPED", "未匹配"],
          ["CONFLICT", "冲突"],
          ["IGNORED", "已忽略"],
        ].map(([k, n]) => (
          <article key={k}>
            <span>{n}</span>
            <b>
              {summary.counts?.find((r: Row) => r.status === k)?.count || 0}
            </b>
          </article>
        ))}
      </div>
      {summary.lastRun?.[0] && (
        <div className="pm-notice">
          最近同步：{summary.lastRun[0].status} · 成功{" "}
          {summary.lastRun[0].succeeded} / 处理 {summary.lastRun[0].processed} ·
          失败 {summary.lastRun[0].failed} ·{" "}
          {summary.lastRun[0].finished || summary.lastRun[0].started}{" "}
          {summary.lastRun[0].error}
        </div>
      )}
      <nav className="wb-center-tabs">
        {[
          ["spus", "SPU / 系列"],
          ["skus", "内部 SKU"],
          ["listings", "WB Listing"],
          ["unmapped", "未匹配商品"],
          ["conflict", "冲突商品"],
          ["tools", "批量工具"],
        ].map(([k, n]) => (
          <button
            key={k}
            className={tab === k ? "active" : ""}
            onClick={() => {
              setTab(k);
              setPage(0);
              setSelected([]);
              setFilters({ shopId: filters.shopId || "" });
            }}
          >
            {n}
          </button>
        ))}
      </nav>
      <div className="sync-actions pm-filters">
        <input
          placeholder="SKU / 名称 / 系列 / nmID / chrtID / 条码"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setPage(0);
          }}
        />
        <select
          aria-label="店铺"
          value={filters.shopId || ""}
          onChange={(e) => setFilter("shopId", e.target.value)}
        >
          <option value="">全部店铺</option>
          {shops.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        {!["spus", "tools"].includes(tab) && (
          <>
            <select
              aria-label="系列"
              value={filters.spuId || ""}
              onChange={(e) => setFilter("spuId", e.target.value)}
            >
              <option value="">全部系列</option>
              {data.spus.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.code} · {s.name}
                </option>
              ))}
            </select>
            {["brand", "category", "supplier"].map((k) => (
              <input
                key={k}
                placeholder={labels[k]}
                value={filters[k] || ""}
                onChange={(e) => setFilter(k, e.target.value)}
              />
            ))}
          </>
        )}
        {!["unmapped", "conflict", "tools"].includes(tab) && (
          <select
            aria-label="状态"
            value={filters.status || ""}
            onChange={(e) => setFilter("status", e.target.value)}
          >
            <option value="">全部状态</option>
            {(kind === "listings"
              ? [
                  "MAPPED",
                  "UNMAPPED",
                  "CONFLICT",
                  "IGNORED",
                  "INVALID",
                  "PARTIAL",
                ]
              : ["active", "draft", "inactive", "archived"]
            ).map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
        )}
        {kind === "listings" && (
          <select
            aria-label="数据质量"
            value={filters.quality || ""}
            onChange={(e) => setFilter("quality", e.target.value)}
          >
            <option value="">全部数据质量</option>
            {["complete", "partial", "missing", "stale"].map((q) => (
              <option key={q}>{q}</option>
            ))}
          </select>
        )}
        <select
          aria-label="排序"
          value={filters.sort || ""}
          onChange={(e) => setFilter("sort", e.target.value)}
        >
          <option value="">默认排序</option>
          <option value="created">创建时间</option>
          <option value="updated">修改时间</option>
          <option value="mapping">映射状态</option>
        </select>
        <button onClick={refresh}>刷新</button>
      </div>
      {tab === "tools" ? (
        <article className="wb-panel">
          <h2>导入商品与映射</h2>
          <p>
            CSV 使用 UTF-8 编码；XLSX
            读取第一个工作表。预览不会写入商品，全部校验通过后才能提交。
          </p>
          <div className="sync-actions">
            <select
              value={importKind}
              onChange={(e) => {
                setImportKind(e.target.value);
                setImportResult(null);
              }}
            >
              <option value="products">商品资料</option>
              <option value="mapping">Listing 绑定</option>
            </select>
            <button
              onClick={() =>
                download(
                  "商品导入模板.csv",
                  importKind === "products"
                    ? "internal_sku_code,spu_code,name_cn,name_ru,brand,category,color,size,width_cm,length_cm,height_cm,weight_kg,default_purchase_cost,purchase_currency,supplier_code\r\n"
                    : "shop,nmID,chrtID,vendorCode,barcode,internalSkuCode\r\n",
                )
              }
            >
              下载 CSV 模板
            </button>
            <input
              aria-label="选择导入文件"
              type="file"
              accept=".csv,.xlsx"
              disabled={!admin || busy}
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void upload(f);
                e.target.value = "";
              }}
            />
            <button
              disabled={!canEdit || busy}
              onClick={() =>
                void run("reprocess", { shopId: filters.shopId || "" })
              }
            >
              重新处理历史商品引用
            </button>
            <button
              onClick={() =>
                void call<Row>("duplicates")
                  .then((r) => {
                    download("重复标识报告.csv", csv(r.rows));
                    setNotice("重复标识报告已导出");
                  })
                  .catch((e) => setError(String(e)))
              }
            >
              导出重复标识报告
            </button>
          </div>
          {importResult && (
            <>
              <h3>
                校验通过 {importResult.valid} / {importResult.total} 行
              </h3>
              <div className="sync-actions">
                <button
                  onClick={() =>
                    download(
                      "导入校验报告.csv",
                      csv(
                        importResult.rows.map((r: Row) => ({
                          row: r.row,
                          status: r.status,
                          message: r.message,
                        })),
                      ),
                    )
                  }
                >
                  下载校验报告
                </button>
                <button
                  className="primary"
                  disabled={
                    !admin || busy || importResult.valid !== importResult.total
                  }
                  onClick={() =>
                    void run("import_commit", { id: importResult.id }, () =>
                      setImportResult(null),
                    )
                  }
                >
                  确认提交整批
                </button>
              </div>
              <div className="pm-scroll">
                <table>
                  <thead>
                    <tr>
                      <th>行</th>
                      <th>结果</th>
                      <th>说明</th>
                    </tr>
                  </thead>
                  <tbody>
                    {importResult.rows.map((r: Row) => (
                      <tr key={r.row}>
                        <td>{r.row}</td>
                        <td>{r.status}</td>
                        <td>{r.message || "校验通过"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </article>
      ) : (
        <article className="wb-panel">
          {kind === "skus" && admin && (
            <div className="sync-actions">
              <span>已选 {selected.length} 项</span>
              <select
                value={bulkField}
                onChange={(e) => {
                  setBulkField(e.target.value);
                  setBulkValue("");
                }}
              >
                <option value="status">修改状态</option>
                <option value="spuId">分配系列</option>
                <option value="category">分配类目</option>
                <option value="supplier">分配供应商</option>
              </select>
              {bulkField === "spuId" ? (
                <select
                  value={bulkValue}
                  onChange={(e) => setBulkValue(e.target.value)}
                >
                  <option value="">未分组</option>
                  {data.spus.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.code}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  aria-label="批量值"
                  placeholder={
                    bulkField === "status"
                      ? "active / draft / inactive / archived"
                      : "新值"
                  }
                  value={bulkValue}
                  onChange={(e) => setBulkValue(e.target.value)}
                />
              )}
              <button
                disabled={!selected.length || busy}
                onClick={() =>
                  void run(
                    "bulk_update",
                    { ids: selected, field: bulkField, value: bulkValue },
                    () => setSelected([]),
                  )
                }
              >
                应用到所选项
              </button>
              <button
                onClick={() => download("当前页商品.csv", csv(data.rows))}
              >
                导出当前页
              </button>
            </div>
          )}
          <table>
            <thead>
              <tr>
                {kind === "skus" && (
                  <th>
                    <input
                      aria-label="选择当前页"
                      type="checkbox"
                      checked={
                        !!data.rows.length &&
                        selected.length === data.rows.length
                      }
                      onChange={(e) =>
                        setSelected(
                          e.target.checked ? data.rows.map((r) => r.id) : [],
                        )
                      }
                    />
                  </th>
                )}
                <th>编码 / 平台标识</th>
                <th>名称 / 变体</th>
                <th>{kind === "listings" ? "店铺 / 条码" : "系列 / 关联"}</th>
                <th>状态</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r) => (
                <tr key={r.id}>
                  {kind === "skus" && (
                    <td>
                      <input
                        aria-label={`选择 ${r.code}`}
                        type="checkbox"
                        checked={selected.includes(r.id)}
                        onChange={(e) =>
                          setSelected(
                            e.target.checked
                              ? [...selected, r.id]
                              : selected.filter((x) => x !== r.id),
                          )
                        }
                      />
                    </td>
                  )}
                  <td>
                    <strong>{r.code || r.nmId}</strong>
                    {kind === "listings" && (
                      <small>
                        nmID {r.nmId}
                        <br />
                        chrtID {r.chrtId}
                        <br />
                        {r.vendorCode}
                      </small>
                    )}
                  </td>
                  <td>
                    {r.name || "—"}
                    <small>
                      {[r.color, r.size].filter(Boolean).join(" · ")}
                    </small>
                  </td>
                  <td>
                    {kind === "listings" ? (
                      <>
                        {r.shop}
                        <small>{r.barcodes || "条码缺失"}</small>
                      </>
                    ) : (
                      <>
                        {r.spu || "—"}
                        <small>
                          {r.variants ?? r.listings ?? 0}{" "}
                          {kind === "spus" ? "个变体" : "个 Listing"}
                        </small>
                      </>
                    )}
                  </td>
                  <td>
                    <span className="status">
                      {r.mappingStatus || r.status}
                    </span>
                    <small>{r.dataStatus}</small>
                  </td>
                  <td>
                    <div className="pm-actions">
                      <button onClick={() => void show(r)}>详情</button>
                      {kind === "listings" && canEdit && (
                        <button
                          onClick={() => {
                            setBinding(r);
                            setPreview(null);
                            setReason("");
                            setSkuQuery("");
                          }}
                        >
                          绑定 / 重绑
                        </button>
                      )}
                      {kind === "listings" && admin && (
                        <>
                          <button onClick={() => void createFrom(r)}>
                            创建 SKU
                          </button>
                          <button
                            onClick={() => {
                              setBinding({ ...r, ignore: true });
                              setReason("");
                              setPreview(null);
                            }}
                          >
                            忽略
                          </button>
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
              {!data.rows.length && (
                <tr>
                  <td colSpan={6} className="pm-empty">
                    {busy
                      ? "正在加载…"
                      : tab === "unmapped"
                        ? "没有未匹配商品。"
                        : tab === "conflict"
                          ? "没有冲突商品。"
                          : "暂无资料，请创建 SKU 或同步店铺商品。"}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          <div className="pm-pagination">
            <span>
              共 {data.total} 条 · 第 {page + 1} 页
            </span>
            <button
              disabled={page === 0 || busy}
              onClick={() => {
                setPage(page - 1);
                setSelected([]);
              }}
            >
              上一页
            </button>
            <button
              disabled={(page + 1) * 50 >= data.total || busy}
              onClick={() => {
                setPage(page + 1);
                setSelected([]);
              }}
            >
              下一页
            </button>
          </div>
        </article>
      )}
      {form && (
        <div className="wb-modal">
          <form
            className="pm-form"
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <h2>{form.action.includes("spu") ? "系列资料" : "SKU 资料"}</h2>
            <div className="pm-fields">
              {Object.entries(form)
                .filter(([k]) => !["action", "id", "listingId"].includes(k))
                .map(([k, v]) => (
                  <label
                    key={k}
                    className={k.endsWith("Text") ? "pm-wide" : ""}
                  >
                    {labels[k] || k}
                    {k === "spuId" ? (
                      <select
                        value={String(v)}
                        onChange={(e) =>
                          setForm({ ...form, [k]: e.target.value })
                        }
                      >
                        <option value="">未分组（可先在主页面创建系列）</option>
                        {data.spus.map((s) => (
                          <option key={s.id} value={s.id}>
                            {s.code} · {s.name}
                          </option>
                        ))}
                      </select>
                    ) : k.endsWith("Text") ? (
                      <PropertyEditor
                        value={String(v)}
                        media={k === "mediaText"}
                        onChange={(value) => setForm({ ...form, [k]: value })}
                      />
                    ) : (
                      <input
                        required={k === "code"}
                        value={String(v ?? "")}
                        onChange={(e) =>
                          setForm({ ...form, [k]: e.target.value })
                        }
                      />
                    )}
                  </label>
                ))}
            </div>
            {error && <p className="wb-center-error">{error}</p>}
            <div>
              <button type="button" onClick={() => setForm(null)}>
                取消
              </button>
              <button className="primary" disabled={busy}>
                保存
              </button>
            </div>
          </form>
        </div>
      )}
      {binding && (
        <div className="wb-modal">
          <div className="wb-panel pm-dialog">
            <h2>{binding.ignore ? "忽略商品" : "绑定内部 SKU"}</h2>
            <p>
              {binding.name} · nmID {binding.nmId} · chrtID {binding.chrtId}
            </p>
            {!binding.ignore && (
              <>
                <input
                  aria-label="搜索内部 SKU"
                  placeholder="搜索内部 SKU 编码或名称"
                  value={skuQuery}
                  onChange={(e) => setSkuQuery(e.target.value)}
                />
                <div className="pm-candidates">
                  {candidates.map((s) => (
                    <button
                      key={s.id}
                      onClick={() =>
                        void call<Row>("preview_bind", {
                          listingId: binding.id,
                          skuId: s.id,
                        })
                          .then(setPreview)
                          .catch((e) => setError(String(e)))
                      }
                    >
                      {s.code} · {s.name} · {s.color} {s.size}{" "}
                      {s.suggestion && "（建议，需人工核对）"}
                    </button>
                  ))}
                </div>
                {preview && (
                  <div className="pm-notice">
                    <b>
                      即将绑定：{preview.sku.code} · {preview.sku.name}
                    </b>
                    <p>
                      颜色 {preview.sku.color || "—"} · 尺寸{" "}
                      {preview.sku.size || "—"} · Vendor{" "}
                      {preview.listing.vendor_code || "—"}
                    </p>
                    {preview.conflicts.length ? (
                      <>
                        <p>
                          存在 {preview.conflicts.length}{" "}
                          个标识冲突。请核实尺寸并填写处理原因；冲突标识仍不会自动供其他业务解析。
                        </p>
                        {preview.conflicts.map((r: Row) => (
                          <p key={r.id}>
                            {r.reason} · nmID {r.nmId} · chrtID {r.chrtId}
                          </p>
                        ))}
                      </>
                    ) : (
                      <p>未检测到标识冲突。</p>
                    )}
                  </div>
                )}
              </>
            )}
            <label>
              操作原因
              <textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </label>
            {error && <p className="wb-center-error">{error}</p>}
            <div className="sync-actions">
              <button onClick={() => setBinding(null)}>取消</button>
              {binding.skuId && admin && !binding.ignore && (
                <button
                  disabled={!reason || busy}
                  onClick={() =>
                    void run("unbind", { listingId: binding.id, reason }, () =>
                      setBinding(null),
                    )
                  }
                >
                  解除绑定
                </button>
              )}
              <button
                className="primary"
                disabled={
                  busy ||
                  (!binding.ignore && !preview) ||
                  ((binding.ignore ||
                    binding.skuId ||
                    preview?.conflicts.length) &&
                    !reason)
                }
                onClick={() =>
                  void run(
                    binding.ignore ? "ignore" : "bind",
                    { listingId: binding.id, skuId: preview?.sku.id, reason },
                    () => setBinding(null),
                  )
                }
              >
                确认{binding.ignore ? "忽略" : "绑定"}
              </button>
            </div>
          </div>
        </div>
      )}
      {detail && (
        <div className="wb-modal">
          <div className="wb-panel pm-dialog pm-detail">
            <div className="panel-title">
              <h2>
                {detail.item.code || detail.item.nm_id} ·{" "}
                {detail.item.name || detail.item.title}
              </h2>
              <div>
                {canEdit &&
                  detail.kind !== "listings" &&
                  (admin || detail.kind === "skus") && (
                    <button onClick={() => edit(detail)}>编辑 / 归档</button>
                  )}
                <button onClick={() => setDetail(null)}>关闭</button>
              </div>
            </div>
            <nav className="wb-center-tabs">
              {[
                "概览",
                ...(detail.kind === "spus" ? ["变体"] : []),
                "平台关联",
                "属性",
                "媒体",
                "历史",
                "后续业务",
              ].map((t) => (
                <button
                  key={t}
                  className={detailTab === t ? "active" : ""}
                  onClick={() => setDetailTab(t)}
                >
                  {t}
                </button>
              ))}
            </nav>
            {detailTab === "概览" && (
              <dl>
                {Object.entries(detail.item)
                  .filter(([k]) => !k.endsWith("_json") && k !== "metadata")
                  .map(([k, v]) => (
                    <div className="pm-dl-row" key={k}>
                      <dt>
                        {labels[k] ||
                          (
                            {
                              spu_id: "系列 ID",
                              purchase_cost: "采购成本",
                              vendor_code: "Vendor Code",
                              nm_id: "nmID",
                              chrt_id: "chrtID",
                              shop_id: "店铺 ID",
                              mapping_status: "映射状态",
                              data_status: "数据质量",
                              last_synced_at: "最近同步",
                              organization_id: "组织",
                              created_at: "创建时间",
                              updated_at: "更新时间",
                            } as Row
                          )[k] ||
                          k}
                      </dt>
                      <dd>{String(v ?? "—")}</dd>
                    </div>
                  ))}
              </dl>
            )}
            {detailTab === "变体" && (
              <table>
                <thead>
                  <tr>
                    <th>SKU</th>
                    <th>颜色</th>
                    <th>尺寸</th>
                    <th>Listing 数</th>
                    <th>状态</th>
                  </tr>
                </thead>
                <tbody>
                  {detail.variants.map((v: Row) => (
                    <tr key={v.id}>
                      <td>
                        <button onClick={() => void show(v, "skus")}>
                          {v.code}
                        </button>
                      </td>
                      <td>{v.color || "—"}</td>
                      <td>{v.size || "—"}</td>
                      <td>{v.listings}</td>
                      <td>{v.status}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {detailTab === "平台关联" &&
              detail.listings.map((l: Row) => (
                <article className="pm-notice" key={l.id}>
                  <b>
                    {l.shop} · nmID {l.nmId} · chrtID {l.chrtId}
                  </b>
                  <p>
                    {l.vendorCode} · 条码 {l.barcodes || "缺失"}
                  </p>
                  <p>
                    {l.mappingStatus} · {l.confidence} · 数据 {l.dataStatus} ·{" "}
                    {l.lastSync}
                  </p>
                  <details>
                    <summary>平台原始字段快照</summary>
                    <pre>{JSON.stringify(l.snapshot, null, 2)}</pre>
                  </details>
                </article>
              ))}
            {detailTab === "属性" && (
              <>
                <h3>内部属性</h3>
                <dl>
                  {Object.entries({
                    ...Object.fromEntries(
                      Object.entries(detail.item.metadata || {}).filter(([k]) =>
                        [
                          "nameRu",
                          "widthCm",
                          "lengthCm",
                          "heightCm",
                          "weightKg",
                          "packSize",
                          "purchaseCurrency",
                        ].includes(k),
                      ),
                    ),
                    ...JSON.parse(detail.item.attributes_json || "{}"),
                  }).map(([k, v]) => (
                    <div className="pm-dl-row" key={k}>
                      <dt>{labels[k] || k}</dt>
                      <dd>
                        {typeof v === "object"
                          ? JSON.stringify(v)
                          : String(v || "—")}
                      </dd>
                    </div>
                  ))}
                </dl>
                {detail.listings.map((l: Row) => (
                  <details key={l.id}>
                    <summary>{l.shop} 平台属性</summary>
                    <pre>
                      {JSON.stringify(
                        l.snapshot.card.characteristics || [],
                        null,
                        2,
                      )}
                    </pre>
                  </details>
                ))}
              </>
            )}
            {detailTab === "媒体" && (
              <div className="pm-media">
                {JSON.parse(detail.item.media_json || "[]").map(
                  (m: Row, i: number) => (
                    <figure key={i}>
                      <img src={m.url} alt="商品媒体" />
                      <figcaption>
                        {m.source || "ERP"} {m.isPrimary ? "· 主图" : ""}
                      </figcaption>
                    </figure>
                  ),
                )}
                {detail.listings.flatMap((l: Row) =>
                  (l.snapshot.card.photos || []).map((m: Row, i: number) => (
                    <figure key={l.id + i}>
                      <img src={m.big || m.c516x688} alt="WB 商品图片" />
                      <figcaption>WILDBERRIES · {l.shop}</figcaption>
                    </figure>
                  )),
                )}
              </div>
            )}
            {detailTab === "历史" && (
              <table>
                <thead>
                  <tr>
                    <th>时间 / 操作者</th>
                    <th>事件</th>
                    <th>原因与变化</th>
                  </tr>
                </thead>
                <tbody>
                  {detail.history.map((h: Row, i: number) => (
                    <tr key={i}>
                      <td>
                        {h.time}
                        <small>{h.actor}</small>
                      </td>
                      <td>{h.event}</td>
                      <td>
                        {h.reason}
                        <details>
                          <summary>查看前后值</summary>
                          <pre>
                            {h.before}
                            <br />
                            {h.after}
                          </pre>
                        </details>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {detailTab === "后续业务" && (
              <p>
                价格、库存、订单、广告、成本和采购将在对应模块接入后，通过内部
                SKU 关联展示。目前数值为 N/A。
              </p>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
