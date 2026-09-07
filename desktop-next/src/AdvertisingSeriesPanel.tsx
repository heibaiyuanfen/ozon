import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openExperiment } from "./AdExperimentCenter";
import {
  exportProductAnalysisJson,
  saveProductSeries,
  seriesInsights,
} from "./bridge";
import type { InsightRow } from "./types";
import "./advertising-series.css";

type Product = { sku: string; offerId: string; name: string };
type Metrics = {
  totalUnits: number | null;
  totalRevenue: number | null;
  impressions: number | null;
  clicks: number | null;
  adOrders: number | null;
  spend: number | null;
  adRevenue: number | null;
  acos: number | null;
  tacos: number | null;
  roas: number | null;
  missingMetrics: string[];
  referenceMetrics: Record<string, number | null>;
  ratioStatus: Record<string, string>;
  salesStatus: string;
  advertisingStatus: string;
};
type Daily = Metrics & { date: string };
type Dataset = {
  schemaVersion: string;
  currency: string;
  generatedAt: string;
  period: { dateFrom: string; dateTo: string; dayCount: number; type?: string };
  series: { name: string; skus: string[]; skuCount: number };
  products: (Product & { daily: Daily[]; summary: Metrics })[];
  seriesDaily: Daily[];
  seriesSummary: Metrics;
  dataQuality: {
    warnings: string[];
    missingData: {
      date: string;
      sku: string;
      offerId: string;
      metrics: string[];
      reasons: Record<string, string>;
    }[];
  };
};
function day(d: Date) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
function recent(n: number) {
  const end = new Date();
  end.setDate(end.getDate() - 1);
  const start = new Date(end);
  start.setDate(start.getDate() - n + 1);
  return { from: day(start), to: day(end) };
}
const metricNames: Record<string, string> = {
  totalUnits: "销量",
  totalRevenue: "销售额",
  impressions: "曝光",
  clicks: "点击",
  adOrders: "广告订单",
  spend: "广告费",
  adRevenue: "广告销售额",
};
const reasonNames: Record<string, string> = {
  sales_record_missing: "销量源记录缺失",
  source_field_missing_or_invalid: "源报告字段未返回或无效",
  downloaded_reports_no_sku_row: "已下载的相关历史报告未包含此 SKU；未确认是否未投放",
  report_coverage_unverified: "报告覆盖尚未核验，请同步所选期限广告",
};
const fmt = (n: number | null) =>
  n == null ? "—" : n.toLocaleString("zh-CN", { maximumFractionDigits: 4 });

export function AdvertisingSeriesPanel({
  shopId,
  selection,
  setSelection,
}: {
  shopId: string;
  selection: Set<string>;
  setSelection: (s: Set<string>) => void;
}) {
  const [catalog, setCatalog] = useState<Product[]>([]),
    [saved, setSaved] = useState<InsightRow[]>([]);
  const [query, setQuery] = useState(""),
    [name, setName] = useState(""),
    [mode, setMode] = useState("last_7_days");
  const [range, setRange] = useState(() => recent(7)),
    [month, setMonth] = useState(() => {
      const d = new Date();
      d.setDate(1);
      d.setMonth(d.getMonth() - 1);
      return day(d).slice(0, 7);
    });
  const [report, setReport] = useState<{ key: string; data: Dataset } | null>(
    null,
  );
  const [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [focus, setFocus] = useState("series");
  const [loading, setLoading] = useState(true);
  const request = useRef(0);
  const key = JSON.stringify([
    shopId,
    [...selection].sort(),
    range.from,
    range.to,
    name.trim(),
    mode,
  ]);
  const currentKey = useRef(key);
  currentKey.current = key;
  const data = report?.key === key ? report.data : null;
  useEffect(() => {
    let alive = true;
    setLoading(true);
    Promise.all([
      invoke<Product[]>("advertising_series_candidates"),
      seriesInsights(range.to),
    ])
      .then(([p, s]) => {
        if (alive) {
          setCatalog(p);
          setSaved(s);
        }
      })
      .catch((e) => {
        if (alive) setMessage(`加载产品或系列失败：${String(e)}`);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
      request.current++;
    };
  }, [shopId]);
  function toggle(sku: string) {
    const next = new Set(selection);
    next.has(sku) ? next.delete(sku) : next.add(sku);
    setSelection(next);
  }
  function chooseMode(value: string) {
    setMode(value);
    if (value === "last_7_days" || value === "last_30_days")
      setRange(recent(value === "last_7_days" ? 7 : 30));
    if (value === "calendar_month") chooseMonth(month);
  }
  function chooseMonth(value: string) {
    setMonth(value);
    setMode("calendar_month");
    if (/^\d{4}-\d{2}$/.test(value)) {
      const [y, m] = value.split("-").map(Number);
      setRange({ from: `${value}-01`, to: day(new Date(y, m, 0)) });
    }
  }
  async function calculate() {
    if (!selection.size || !range.from || !range.to || range.from > range.to) {
      setMessage("请选择产品及有效起止日期");
      return;
    }
    const token = ++request.current;
    const snapshot = key;
    setBusy(true);
    setMessage("");
    try {
      const result = await invoke<Dataset>("advertising_series_dataset", {
        range,
        skus: [...selection],
        name,
        shopId,
      });
      if (token === request.current && currentKey.current === snapshot) {
        result.period.type = mode;
        setReport({ key: snapshot, data: result });
        setFocus("series");
        setMessage(
          `已生成 ${result.series.skuCount} 个产品 × ${result.period.dayCount} 天的明细；请留意数据完整性说明。`,
        );
      }
    } catch (e) {
      if (token === request.current) setMessage(`计算失败：${String(e)}`);
    } finally {
      if (token === request.current) setBusy(false);
    }
  }
  async function syncRange() {
    if (!selection.size || !range.from || !range.to || range.from > range.to)
      return;
    const token = ++request.current,
      snapshot = key;
    setBusy(true);
    setReport(null);
    setMessage(
      "正在同步所选期限广告并获取历史商品明细，报告生成可能需要几分钟…",
    );
    let warning = "";
    try {
      try {
        await invoke<number>("sync_performance_ads", { range, force: false });
      } catch (e) {
        warning = `同步未全部完成：${String(e)}。下方仍展示已保存数据。`;
      }
      if (request.current !== token || currentKey.current !== snapshot) return;
      const result = await invoke<Dataset>("advertising_series_dataset", {
        range,
        skus: [...selection],
        name,
        shopId,
      });
      if (request.current !== token || currentKey.current !== snapshot) return;
      result.period.type = mode;
      setReport({ key: snapshot, data: result });
      setFocus("series");
      setMessage(
        warning ||
          "同步与预览已更新。存在缺失字段时会保留有效数据，请查看缺失清单。",
      );
    } catch (e) {
      if (request.current === token) setMessage(`预览失败：${String(e)}`);
    } finally {
      if (request.current === token) setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    try {
      await saveProductSeries(null, name.trim(), [...selection]);
      setSaved(await seriesInsights(range.to));
      setMessage(`已保存系列“${name.trim()}”`);
    } catch (e) {
      setMessage(`保存失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  }
  async function exportJson() {
    if (!data) return;
    setBusy(true);
    try {
      const path = await exportProductAnalysisJson(
        `ozon_ad_series_${shopId.replace(/[^a-zA-Z0-9_-]/g, "_")}_${range.from}_${range.to}_${Date.now()}`,
        data,
      );
      setMessage(`JSON 已导出：${path}`);
    } catch (e) {
      setMessage(`导出失败：${String(e)}`);
    } finally {
      setBusy(false);
    }
  }
  const visible = catalog.filter((p) =>
    `${p.sku} ${p.offerId} ${p.name}`
      .toLowerCase()
      .includes(query.trim().toLowerCase()),
  );
  const rows = data
    ? focus === "series"
      ? data.seriesDaily
      : data.products.find((p) => p.sku === focus)?.daily || []
    : [];
  const summary = data
    ? focus === "series"
      ? data.seriesSummary
      : data.products.find((p) => p.sku === focus)?.summary
    : null;
  return (
    <section className="card ad-series-v2">
      <div className="section-heading">
        <div>
          <h2>产品系列 · 每日广告与销量</h2>
          <p>选择产品组成系列，导出每个产品与整个系列的逐日数据和周期合计。</p>
        </div>
        <span>JSON v2.2</span>
        <button disabled={!selection.size} onClick={() => openExperiment({skus:[...selection],name:"所选广告系列"})}>创建广告实验</button>
      </div>
      <fieldset disabled={busy}>
        <div className="series-controls">
          <label>
            系列名称
            <input
              aria-label="系列名称"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：ABC 产品系列"
            />
          </label>
          <button
            disabled={!name.trim() || !selection.size}
            onClick={() => void save()}
          >
            保存为系列
          </button>
        </div>
        <div className="series-chips">
          {saved.map((s) => (
            <button
              key={s.id}
              onClick={() => {
                setSelection(new Set(s.skus));
                setName(s.name);
              }}
            >
              {s.name} · {s.skus.length} 个产品
            </button>
          ))}
        </div>
        <div className="series-controls">
          <label>
            统计期限
            <select
              aria-label="统计期限"
              value={mode}
              onChange={(e) => chooseMode(e.target.value)}
            >
              <option value="last_7_days">最近 7 天（截止昨日）</option>
              <option value="last_30_days">最近 30 天（截止昨日）</option>
              <option value="calendar_month">自然月</option>
              <option value="custom">自选起止日期</option>
            </select>
          </label>
          {mode === "calendar_month" && (
            <label>
              月份
              <input
                aria-label="统计月份"
                type="month"
                value={month}
                onChange={(e) => chooseMonth(e.target.value)}
              />
            </label>
          )}
          <label>
            开始日期
            <input
              aria-label="开始日期"
              type="date"
              value={range.from}
              onChange={(e) => {
                setMode("custom");
                setRange({ ...range, from: e.target.value });
              }}
            />
          </label>
          <label>
            结束日期
            <input
              aria-label="结束日期"
              type="date"
              value={range.to}
              onChange={(e) => {
                setMode("custom");
                setRange({ ...range, to: e.target.value });
              }}
            />
          </label>
        </div>
        <div className="series-controls">
          <input
            aria-label="搜索系列产品"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索全部产品：名称 / SKU / 货号"
          />
          <span>
            已选 {selection.size} 个产品 · 匹配 {visible.length} 个
          </span>
          <button
            onClick={() =>
              setSelection(
                new Set([...selection, ...visible.map((p) => p.sku)]),
              )
            }
            disabled={
              new Set([...selection, ...visible.map((p) => p.sku)]).size > 500
            }
          >
            全选搜索结果
          </button>
          <button onClick={() => setSelection(new Set())}>清空选择</button>
        </div>
        <div className="series-product-list">
          {loading ? (
            <p>正在加载产品…</p>
          ) : visible.length ? (
            visible.map((p) => (
              <label key={p.sku}>
                <input
                  type="checkbox"
                  aria-label={`系列产品 ${p.sku}`}
                  checked={selection.has(p.sku)}
                  onChange={() => toggle(p.sku)}
                />
                <span>
                  <b>{p.offerId || p.sku}</b> {p.name}
                  <small>SKU：{p.sku}</small>
                </span>
              </label>
            ))
          ) : (
            <p>没有匹配产品。请先同步商品或销售数据。</p>
          )}
        </div>
        <div className="series-chips">
          {[...selection].map((sku) => (
            <button key={sku} onClick={() => toggle(sku)} title="点击移除">
              {catalog.find((p) => p.sku === sku)?.offerId || sku} ×
            </button>
          ))}
        </div>
      </fieldset>
      <div className="series-controls">
        <button
          className="primary"
          disabled={busy || !selection.size || selection.size > 500}
          onClick={() => void calculate()}
        >
          {busy ? "处理中…" : "生成数据预览"}
        </button>
        <button
          disabled={busy || !selection.size}
          onClick={() => void syncRange()}
        >
          同步所选期限广告
        </button>
        <button disabled={busy || !data} onClick={() => void exportJson()}>
          导出 JSON
        </button>
        <span>
          {range.from} 至 {range.to}（包含起止日）
        </span>
      </div>
      {message && (
        <p className="sync-message" role="status">
          {message}
        </p>
      )}
      {report && !data && (
        <p className="series-warning">
          产品、名称或期限已变化，请重新生成预览后导出。
        </p>
      )}
      {data && (
        <>
          <div className="series-warning">
            {data.dataQuality.warnings.map((w) => (
              <p key={w}>{w}</p>
            ))}
          </div>
          <div className="series-controls">
            <label>
              查看明细
              <select
                aria-label="查看明细"
                value={focus}
                onChange={(e) => setFocus(e.target.value)}
              >
                <option value="series">系列合计（全部所选产品）</option>
                {data.products.map((p) => (
                  <option key={p.sku} value={p.sku}>
                    {p.offerId || p.sku} · {p.name}
                  </option>
                ))}
              </select>
            </label>
            <span>
              {data.period.dayCount} 天 · {data.currency} ·
              “—”表示数据缺失或不可计算
            </span>
          </div>
          <div className="series-table">
            <table>
              <thead>
                <tr>
                  <th>日期</th>
                  <th>销量（件）</th>
                  <th>销售额</th>
                  <th>曝光</th>
                  <th>点击</th>
                  <th>广告订单</th>
                  <th>广告费</th>
                  <th>广告销售额</th>
                  <th>ACOS %</th>
                  <th>TACOS %</th>
                  <th>ROAS</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.date}>
                    <td>{r.date}</td>
                    {(
                      [
                        "totalUnits",
                        "totalRevenue",
                        "impressions",
                        "clicks",
                        "adOrders",
                        "spend",
                        "adRevenue",
                        "acos",
                        "tacos",
                        "roas",
                      ] as const
                    ).map((k) => (
                      <td
                        key={k}
                        title={
                          r.ratioStatus?.[k] === "reference_only" ? "按已知合计计算，分子分母覆盖可能不同，不代表完整业绩" : r.missingMetrics.includes(k)
                            ? "仅合计已知数据，仍有缺失"
                            : undefined
                        }
                      >
                        {fmt(r[k] ?? r.referenceMetrics?.[k] ?? null)}
                        {r.ratioStatus?.[k] === "reference_only" ? "（参考）" : ""}
                        {r[k] != null && r.missingMetrics.includes(k)
                          ? " *"
                          : ""}
                      </td>
                    ))}
                  </tr>
                ))}
                {summary && (
                  <tr className="series-total">
                    <td>周期已知合计</td>
                    {(
                      [
                        "totalUnits",
                        "totalRevenue",
                        "impressions",
                        "clicks",
                        "adOrders",
                        "spend",
                        "adRevenue",
                        "acos",
                        "tacos",
                        "roas",
                      ] as const
                    ).map((k) => (
                      <td
                        key={k}
                        title={
                          summary.ratioStatus?.[k] === "reference_only" ? "按已知合计计算，分子分母覆盖可能不同，不代表完整业绩" : summary.missingMetrics.includes(k)
                            ? "仅合计已知数据，仍有缺失"
                            : undefined
                        }
                      >
                        {fmt(summary[k] ?? summary.referenceMetrics?.[k] ?? null)}
                        {summary.ratioStatus?.[k] === "reference_only" ? "（参考）" : ""}
                        {summary[k] != null &&
                        summary.missingMetrics.includes(k)
                          ? " *"
                          : ""}
                      </td>
                    ))}
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          {data.dataQuality.missingData.length > 0 && (
            <details open>
              <summary>
                缺失清单（{data.dataQuality.missingData.length} 个产品日期；*
                表示已知合计不完整）
              </summary>
              <div
                className="series-table"
                style={{ maxHeight: 240, overflow: "auto" }}
              >
                <table>
                  <thead>
                    <tr>
                      <th>日期</th>
                      <th>产品 / SKU</th>
                      <th>缺失或部分缺失指标</th>
                      <th>核查结果</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.dataQuality.missingData
                      .filter((g) => focus === "series" || g.sku === focus)
                      .map((g) => (
                        <tr key={`${g.date}-${g.sku}`}>
                          <td>{g.date}</td>
                          <td>
                            {g.offerId || g.sku} · {g.sku}
                          </td>
                          <td>
                            {g.metrics
                              .map((k) => metricNames[k] || k)
                              .join("、")}
                          </td>
                          <td>{Array.from(new Set(Object.values(g.reasons || {}))).map(reason => reasonNames[reason] || reason).join("；")}</td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            </details>
          )}
          <details>
            <summary>查看完整 JSON（单品分别列出，包含计算公式）</summary>
            <pre className="series-json">{JSON.stringify(data, null, 2)}</pre>
          </details>
        </>
      )}
    </section>
  );
}
