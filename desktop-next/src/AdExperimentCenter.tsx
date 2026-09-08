import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as echarts from "./charts";
import { seriesInsights, exportProductAnalysisJson } from "./bridge";
import "./ad-experiments.css";

export type ExperimentSeed = {
  skus: string[];
  name?: string;
  seriesId?: number;
  experimentType?: string;
  observationDays?: number;
  notes?: string;
  actions?: Record<string, string>;
};
export function openExperiment(seed: ExperimentSeed) {
  window.dispatchEvent(
    new CustomEvent("create-ad-experiment", { detail: seed }),
  );
}
type Target = {
  dailyUnits: number;
  tacosMax: number;
  cvr: number | null;
  cpa: number | null;
  cpc: number | null;
  acos: number | null;
};
type Targets = {
  stages: Target[];
  finalTarget: Target;
  preferredTacosMin: number;
  preferredTacosMax: number;
  tacosHardLimit: number;
};
type Change = {
  sku: string;
  action: string;
  campaignId: string | null;
  beforeBudget: number | null;
  afterBudget: number | null;
  beforePrice: number | null;
  afterPrice: number | null;
  beforeStatus: string | null;
  afterStatus: string | null;
  reason: string;
};
type Draft = {
  name: string;
  experimentType: string;
  seriesId: number | null;
  baselineStart: string;
  baselineEnd: string;
  observationDays: number;
  operator: string;
  notes: string;
  changes: Change[];
  targets: Targets;
};
type Metric = Record<string, number | null>;
type Evaluation = {
  decision: string;
  confidence: string;
  score: number | null;
  completedDays: number;
  observationDays: number;
  stageQualified: boolean;
  baseline: Metric;
  current: Metric;
  changes: Metric;
  marginal: Metric;
  quality: { status: string; partialDays: number; missingDays: number };
  baselineQuality: { status: string };
  scoreComponents: Metric;
  vetoes: string[];
  stockDays: number | null;
  facts: string[];
  inference: string[];
  nextAction: string;
  daily: MetricDay[];
  baselineDaily: MetricDay[];
  skuScores: (Evaluation & { sku: string })[];
  goals: Metric;
};
type MetricDay = Metric & { date: string };
type Experiment = {
  id: number;
  input: Draft;
  status: string;
  stageIndex: number;
  createdAt: string;
  observationStart: string | null;
  evaluation: Evaluation | null;
  events: {
    id: number;
    time: string;
    sku: string;
    type: string;
    before: string;
    after: string;
    operator: string;
    reason: string;
  }[];
  stableVersions: { id: number; name: string; payload: string }[];
  ai: { text: string; evaluatedAt: string } | null;
};
type Product = { sku: string; offerId: string; name: string };
type Configuration = {
  sku: string;
  price: number | null;
  priceCurrency: string | null;
  campaigns: {
    campaignId: string;
    name: string;
    state: string;
    weeklyBudgetRub: number | null;
  }[];
};
type Context = {
  configuration: Configuration[];
  dataset: { seriesSummary: Metric; dataQuality: { warnings: string[] } };
};
const kinds: Record<string, string> = {
  budget_increase: "预算增加",
  budget_decrease: "预算减少",
  budget_reallocation: "系列预算重新分配",
  pause_test: "暂停广告",
  resume_test: "恢复广告",
  price_test: "价格测试",
  creative_test: "素材测试",
  listing_test: "页面测试",
  promotion_test: "促销测试",
  mixed: "复合实验",
};
const actions: Record<string, string> = {
  increase_budget: "增加预算",
  reduce_budget: "减少预算",
  pause: "暂停",
  resume: "恢复",
  hold: "保持",
  price_change: "调整价格",
  creative_change: "修改图片",
  listing_change: "修改页面",
  promotion_change: "调整促销",
};
const labels: Record<string, string> = {
  draft: "草稿",
  running: "已启动",
  observing: "观察中",
  success: "成功",
  hold: "保持",
  weak_success: "弱有效",
  rollback: "建议回退",
  stopped: "已停止",
  completed: "已完成",
  SUCCESS: "成功",
  HOLD: "保持观察",
  WEAK_SUCCESS: "弱有效",
  ROLLBACK: "建议回退",
  STOP_SCALE: "禁止继续放量",
  INSUFFICIENT_DATA: "数据不足",
  DRAFT: "待启动",
  complete: "完整",
  partial: "部分完整",
  missing: "缺失",
  High: "高",
  Medium: "中",
  Low: "低",
};
const fmt = (v: number | null | undefined, suffix = "") =>
  v == null || !Number.isFinite(v)
    ? "—"
    : `${v.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}${suffix}`;
const date = (offset: number) => {
  const d = new Date();
  d.setDate(d.getDate() + offset);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
const target = (units: number, tacos: number): Target => ({
  dailyUnits: units,
  tacosMax: tacos,
  cvr: null,
  cpa: null,
  cpc: null,
  acos: null,
});
const blankChange = (sku: string): Change => ({
  sku,
  action: "hold",
  campaignId: null,
  beforeBudget: null,
  afterBudget: null,
  beforePrice: null,
  afterPrice: null,
  beforeStatus: null,
  afterStatus: null,
  reason: "",
});
function draftFor(seed?: ExperimentSeed): Draft {
  return {
    name: seed?.name ? `${seed.name} 广告实验` : "",
    experimentType: seed?.experimentType || "budget_reallocation",
    seriesId: seed?.seriesId ?? null,
    baselineStart: date(-3),
    baselineEnd: date(-1),
    observationDays: seed?.observationDays || 3,
    operator: "本地运营人员",
    notes: seed?.notes || "",
    changes: (seed?.skus || []).map(sku => ({ ...blankChange(sku), action: seed?.actions?.[sku] || "hold" })),
    targets: {
      stages: [target(35, 8), target(42, 8), target(45, 9), target(50, 10)],
      finalTarget: target(50, 10),
      preferredTacosMin: 6,
      preferredTacosMax: 8,
      tacosHardLimit: 10,
    },
  };
}
function Numeric({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number | null;
  onChange: (v: number | null) => void;
}) {
  return (
    <label>
      {label}
      <input
        type="number"
        step="any"
        min="0"
        value={value ?? ""}
        placeholder="未知 / 未设置"
        onChange={(e) =>
          onChange(e.target.value === "" ? null : Number(e.target.value))
        }
      />
    </label>
  );
}
function AiSections({text}:{text:string}) {
  try {const value=JSON.parse(text.replace(/^```(?:json)?\s*|\s*```$/g,"")) as Record<string,unknown>;return <>{Object.entries({facts:"事实",inference:"推断",decision:"判断",nextAction:"下一步建议"}).map(([key,title])=><section key={key}><h4>{title}</h4>{(Array.isArray(value[key])?value[key] as unknown[]:[value[key]??"未返回"]).map((s,i)=><p key={i}>{typeof s==="string"?s:JSON.stringify(s)}</p>)}</section>)}</>;}catch{return <pre>{text}</pre>;}
}
function Trend({ value }: { value: Evaluation }) {
  const ref = useRef<HTMLDivElement>(null);
  const [key, setKey] = useState("totalUnits");
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    const rows = [...(value.baselineDaily || []), ...(value.daily || [])];
    chart.setOption({
      tooltip: { trigger: "axis" },
      grid: { left: 65, right: 30, bottom: 45, top: 25 },
      xAxis: { type: "category", data: rows.map((r) => r.date) },
      yAxis: { type: "value" },
      series: [
        {
          type: "line",
          name: key,
          data: rows.map((r) => r[key]),
          connectNulls: false,
          markLine: {
            symbol: "none",
            data: value.daily?.length
              ? [{ xAxis: value.daily[0].date, name: "完整日观察开始" }]
              : [],
          },
          markArea: {
            data: value.baselineDaily?.length
              ? [
                  [
                    { name: "锁定基准", xAxis: value.baselineDaily[0].date },
                    {
                      xAxis:
                        value.baselineDaily[value.baselineDaily.length - 1]
                          .date,
                    },
                  ],
                ]
              : [],
          },
        },
      ],
    });
    const obs = new ResizeObserver(() => chart.resize());
    obs.observe(ref.current);
    return () => {
      obs.disconnect();
      chart.dispose();
    };
  }, [value, key]);
  return (
    <section className="card">
      <div className="exp-toolbar">
        <h3>修改前后趋势</h3>
        <select value={key} onChange={(e) => setKey(e.target.value)}>
          {Object.entries({
            totalUnits: "每日销量",
            spend: "广告费 ₽",
            tacos: "TACOS %",
            conversionRate: "CVR %",
            cpa: "CPA ₽",
          }).map(([k, v]) => (
            <option value={k} key={k}>
              {v}
            </option>
          ))}
        </select>
      </div>
      <p>
        基准区域为启动时快照；曲线空档代表缺失数据。修改时间详见下方时间轴。
      </p>
      <div ref={ref} style={{ height: 270 }} />
    </section>
  );
}

export function AdExperimentCenter({
  shopId,
  seed,
}: {
  shopId: string;
  seed?: ExperimentSeed;
}) {
  const [list, setList] = useState<Experiment[]>([]),
    [selected, setSelected] = useState<Experiment | null>(null),
    [creating, setCreating] = useState(Boolean(seed)),
    [editingId, setEditingId] = useState<number|null>(null),
    [draft, setDraft] = useState<Draft>(() => draftFor(seed)),
    [step, setStep] = useState(0),
    [products, setProducts] = useState<Product[]>([]),
    [series, setSeries] = useState<
      { id: number; name: string; skus: string[] }[]
    >([]),
    [query, setQuery] = useState(""),
    [filter, setFilter] = useState(""),
    [kindFilter, setKindFilter] = useState(""),
    [fromFilter, setFromFilter] = useState(""),
    [context, setContext] = useState<Context | null>(null),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [eventSku, setEventSku] = useState(""),
    [eventBefore, setEventBefore] = useState(""),
    [eventAfter, setEventAfter] = useState(""),
    [eventReason, setEventReason] = useState(""),
    [restore, setRestore] = useState<unknown>(null);
  const live = useRef(true);
  useEffect(() => {live.current=true;return()=>{live.current=false;};}, []);
  const call = <T,>(
    command: string,
    payload: unknown = {},
    id: number | null = null,
  ) => invoke<T>("ad_experiment_command", { shopId, command, id, payload });
  async function reload() {
    const r = await call<Experiment[]>("list");
    if (live.current) {
      setList(r);
      setSelected((old) => (old ? r.find((x) => x.id === old.id) || old : old));
    }
  }
  useEffect(() => {
    let disposed = false;
    Promise.all([
      invoke<Product[]>("advertising_series_candidates"),
      seriesInsights(date(-1)),
      call<Experiment[]>("list"),
    ])
      .then(([p, s, l]) => {
        if (!disposed) {
          setProducts(p);
          setSeries(s.map(x=>({...x,id:Number(x.id)})));
          setList(l);
        }
      })
      .catch((e) => {
        if (!disposed) setMessage(String(e));
      });
    const timer = window.setInterval(() => {
      if (!disposed && !busy)
        void reload().catch((e) => {
          if (!disposed) setMessage(String(e));
        });
    }, 30000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [shopId]);
  useEffect(() => {
    if (seed) {
      setDraft(draftFor(seed));
      setEditingId(null);
      setCreating(true);
      setStep(0);
      setContext(null);
    }
  }, [seed]);
  async function run(command: string, payload: unknown = {}) {
    if (!selected) return;
    if(selected.id<0){setMessage("当前为开发示例，未保存到业务数据库。请创建真实实验后执行操作。");return;}
    setBusy(true);
    setMessage("");
    try {
      const r = await call<Experiment & { restorePlan?: unknown }>(
        command,
        payload,
        selected.id,
      );
      if (!live.current) return;
      if (r.restorePlan) {
        setRestore(r.restorePlan);
        setMessage("恢复清单已生成，尚未修改平台配置。");
      } else setSelected(r);
      await reload();
    } catch (e) {
      if (live.current) setMessage(String(e));
    } finally {
      if (live.current) setBusy(false);
    }
  }
  async function preview() {
    setBusy(true);
    setMessage("");
    try {
      const c = await call<Context>("context", {
        skus: draft.changes.map((x) => x.sku),
        from: draft.baselineStart,
        to: draft.baselineEnd,
      });
      setContext(c);
      setDraft((d) => ({
        ...d,
        changes: d.changes.map((x) => {
          const p = c.configuration.find((p) => p.sku === x.sku);
          const a = p?.campaigns.length === 1 ? p.campaigns[0] : null;
          return {
            ...x,
            beforePrice: p?.price ?? null,
            campaignId: x.campaignId ?? a?.campaignId ?? null,
            beforeBudget: a?.weeklyBudgetRub ?? null,
            beforeStatus: a?.state ?? null,
          };
        }),
      }));
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    setBusy(true);
    setMessage("");
    try {
      const r = await call<Experiment>(editingId==null?"create":"update", draft, editingId);
      setSelected(r);
      setCreating(false);
      await reload();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  }
  const change = (sku: string, patch: Partial<Change>) =>
    setDraft((d) => ({
      ...d,
      changes: d.changes.map((x) => (x.sku === sku ? { ...x, ...patch } : x)),
    }));
  const toggle = (sku: string) => {
    setContext(null);
    setDraft((d) => ({
      ...d,
      seriesId: null,
      changes: d.changes.some((x) => x.sku === sku)
        ? d.changes.filter((x) => x.sku !== sku)
        : [...d.changes, blankChange(sku)],
    }));
  };
  const e = selected?.evaluation;
  const filtered = list.filter(
    (x) =>
      (!filter || x.status === filter || x.evaluation?.decision === filter) &&
      (!kindFilter || x.input.experimentType === kindFilter) &&
      (!fromFilter || x.createdAt.slice(0, 10) >= fromFilter) &&
      `${x.input.name} ${x.input.changes.map((c) => c.sku).join(" ")}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  return (
    <div className="exp-center">
      <section className="card exp-heading">
        <div>
          <span>Action → Measurement → Decision</span>
          <h2>广告优化实验中心</h2>
          <p>记录修改、锁定基准、跟踪完整自然日，并明确下一步行动。</p>
        </div>
        <button
          disabled={busy}
          onClick={() => {
            setDraft(draftFor());
            setEditingId(null);
            setCreating(true);
            setContext(null);
            setStep(0);
          }}
        >
          创建实验
        </button>
        <button disabled={busy} onClick={()=>{void call<Experiment>("fixture").then(x=>{setSelected(x);setCreating(false);setMessage("开发示例：基准来自需求，观察值为假设数据，未写入业务记录。");}).catch(e=>setMessage(String(e)));}}>查看 GJYB001 开发示例</button>
      </section>
      {message && (
        <div role="status" className="exp-message">
          {message}
        </div>
      )}
      {creating ? (
        <section className="card exp-wizard">
          <div className="exp-toolbar">
            <h3>创建实验 · {step + 1} / 7</h3>
            <button onClick={() => setCreating(false)}>关闭向导</button>
          </div>
          <nav className="exp-steps">
            {[
              "选择产品",
              "实验类型",
              "基准快照",
              "修改内容",
              "阶段目标",
              "观察周期",
              "保存草稿",
            ].map((s, i) => (
              <button
                key={s}
                className={step === i ? "active" : ""}
                onClick={() => setStep(i)}
              >
                {i + 1}. {s}
              </button>
            ))}
          </nav>
          {step === 0 && (
            <>
              <label>
                实验名称
                <input
                  value={draft.name}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
              </label>
              <label>
                已有系列
                <select
                  value={draft.seriesId ?? ""}
                  onChange={(e) => {
                    const s = series.find(
                      (x) => x.id === Number(e.target.value),
                    );
                    if (s) {
                      setDraft({
                        ...draft,
                        seriesId: s.id,
                        name: `${s.name} 广告实验`,
                        changes: s.skus.map(blankChange),
                      });
                      setContext(null);
                    }
                  }}
                >
                  <option value="">选择系列或下方 SKU</option>
                  {series.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                </select>
              </label>
              <input
                aria-label="搜索产品"
                placeholder="搜索 SKU、货号或商品名"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <div className="exp-picker">
                {products
                  .filter((p) =>
                    `${p.sku} ${p.offerId} ${p.name}`
                      .toLowerCase()
                      .includes(query.toLowerCase()),
                  )
                  .map((p) => (
                    <label key={p.sku}>
                      <input
                        type="checkbox"
                        checked={draft.changes.some((x) => x.sku === p.sku)}
                        onChange={() => toggle(p.sku)}
                      />
                      <span>
                        {p.offerId || p.sku}
                        <small>
                          {p.sku} · {p.name}
                        </small>
                      </span>
                    </label>
                  ))}
              </div>
              <p>已选 {draft.changes.length} SKU</p>
            </>
          )}
          {step === 1 && (
            <>
              <label>
                实验类型
                <select
                  value={draft.experimentType}
                  onChange={(e) =>
                    setDraft({ ...draft, experimentType: e.target.value })
                  }
                >
                  {Object.entries(kinds).map(([k, v]) => (
                    <option key={k} value={k}>
                      {v}
                    </option>
                  ))}
                </select>
              </label>
              <p>
                普通实验尽量只修改一个主要变量。系列预算分配允许多 SKU
                联动；额外改价或换图会降低因果解释能力。
              </p>
              <label>
                假设与说明
                <textarea
                  value={draft.notes}
                  onChange={(e) =>
                    setDraft({ ...draft, notes: e.target.value })
                  }
                />
              </label>
            </>
          )}
          {step === 2 && (
            <>
              <div className="exp-toolbar">
                {[3, 7, 14].map((n) => (
                  <button
                    key={n}
                    onClick={() => {
                      setDraft({
                        ...draft,
                        baselineStart: date(-n),
                        baselineEnd: date(-1),
                      });
                      setContext(null);
                    }}
                  >
                    最近 {n} 个完整日
                  </button>
                ))}
              </div>
              <div className="exp-fields">
                <label>
                  开始
                  <input
                    type="date"
                    value={draft.baselineStart}
                    max={date(-1)}
                    onChange={(e) => {
                      setDraft({ ...draft, baselineStart: e.target.value });
                      setContext(null);
                    }}
                  />
                </label>
                <label>
                  结束
                  <input
                    type="date"
                    max={date(-1)}
                    value={draft.baselineEnd}
                    onChange={(e) => {
                      setDraft({ ...draft, baselineEnd: e.target.value });
                      setContext(null);
                    }}
                  />
                </label>
                <button
                  disabled={busy || !draft.changes.length}
                  onClick={() => void preview()}
                >
                  读取当前配置与基准预览
                </button>
              </div>
              {context && (
                <>
                  <p>
                    此处是预览；点击启动时永久锁定基准。金额统一采用源币种
                    RUB；商品价格保留自身币种。
                  </p>
                  <div className="exp-kpis">
                    {[
                      "totalUnits",
                      "spend",
                      "tacos",
                      "conversionRate",
                      "cpa",
                      "cpc",
                    ].map((k) => (
                      <div key={k}>
                        <small>
                          {
                            (
                              {
                                totalUnits: "周期销量",
                                spend: "广告费 ₽",
                                tacos: "TACOS %",
                                conversionRate: "CVR %",
                                cpa: "CPA ₽",
                                cpc: "CPC ₽",
                              } as Record<string, string>
                            )[k]
                          }
                        </small>
                        <strong>{fmt(context.dataset.seriesSummary[k])}</strong>
                      </div>
                    ))}
                  </div>
                  <p>{context.dataset.dataQuality.warnings[0]}</p>
                </>
              )}
            </>
          )}
          {step === 3 && (
            <>
              <p>
                预算为广告计划周预算 RUB，共享计划不会按 SKU
                分摊。下方是操作记录，不会立即提交平台；若价格币种不是
                RUB，请按商品原币种填写。
              </p>
              {draft.changes.map((x) => {
                const p = products.find((p) => p.sku === x.sku);
                const config = context?.configuration.find(
                  (p) => p.sku === x.sku,
                );
                return (
                  <article className="exp-change" key={x.sku}>
                    <h4>
                      {p?.offerId || x.sku} · {x.sku}
                    </h4>
                    <div className="exp-fields">
                      <label>
                        动作
                        <select
                          value={x.action}
                          onChange={(e) =>
                            change(x.sku, {
                              action: e.target.value,
                              afterStatus:
                                e.target.value === "pause"
                                  ? "Paused"
                                  : e.target.value === "resume"
                                    ? "Active"
                                    : x.afterStatus,
                            })
                          }
                        >
                          {Object.entries(actions).map(([k, v]) => (
                            <option key={k} value={k}>
                              {v}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        关联计划
                        <select
                          value={x.campaignId ?? ""}
                          onChange={(e) => {
                            const c = config?.campaigns.find(
                              (p) => p.campaignId === e.target.value,
                            );
                            change(x.sku, {
                              campaignId: c?.campaignId ?? null,
                              beforeBudget: c?.weeklyBudgetRub ?? null,
                              beforeStatus: c?.state ?? null,
                            });
                          }}
                        >
                          <option value="">未关联 / 需核对</option>
                          {config?.campaigns.map((c) => (
                            <option key={c.campaignId} value={c.campaignId}>
                              {c.name}（{c.campaignId}）
                            </option>
                          ))}
                        </select>
                      </label>
                      <Numeric
                        label="修改前周预算 ₽"
                        value={x.beforeBudget}
                        onChange={(v) => change(x.sku, { beforeBudget: v })}
                      />
                      <Numeric
                        label="修改后周预算 ₽"
                        value={x.afterBudget}
                        onChange={(v) => change(x.sku, { afterBudget: v })}
                      />
                      <Numeric
                        label={`修改前价格 ${config?.priceCurrency || "原币种"}`}
                        value={x.beforePrice}
                        onChange={(v) => change(x.sku, { beforePrice: v })}
                      />
                      <Numeric
                        label="修改后价格（原币种）"
                        value={x.afterPrice}
                        onChange={(v) => change(x.sku, { afterPrice: v })}
                      />
                      <label>
                        修改前状态
                        <input
                          value={x.beforeStatus ?? ""}
                          onChange={(e) =>
                            change(x.sku, {
                              beforeStatus: e.target.value || null,
                            })
                          }
                        />
                      </label>
                      <label>
                        修改后状态
                        <input
                          value={x.afterStatus ?? ""}
                          onChange={(e) =>
                            change(x.sku, {
                              afterStatus: e.target.value || null,
                            })
                          }
                        />
                      </label>
                    </div>
                    <label>
                      操作原因 / 素材或页面修改说明
                      <input
                        value={x.reason}
                        onChange={(e) =>
                          change(x.sku, { reason: e.target.value })
                        }
                      />
                    </label>
                  </article>
                );
              })}
            </>
          )}
          {step === 4 && (
            <>
              <h4>阶段路径</h4>
              {draft.targets.stages.map((t, i) => (
                <div className="exp-fields" key={i}>
                  <strong>Stage {i + 1}</strong>
                  <Numeric
                    label="日均销量目标（件）"
                    value={t.dailyUnits}
                    onChange={(v) =>
                      setDraft({
                        ...draft,
                        targets: {
                          ...draft.targets,
                          stages: draft.targets.stages.map((x, j) =>
                            i === j ? { ...x, dailyUnits: v ?? 0 } : x,
                          ),
                        },
                      })
                    }
                  />
                  <Numeric
                    label="TACOS 上限 %"
                    value={t.tacosMax}
                    onChange={(v) =>
                      setDraft({
                        ...draft,
                        targets: {
                          ...draft.targets,
                          stages: draft.targets.stages.map((x, j) =>
                            i === j ? { ...x, tacosMax: v ?? 0 } : x,
                          ),
                        },
                      })
                    }
                  />
                  {(["cvr", "cpa", "cpc", "acos"] as const).map((k) => (
                    <Numeric
                      key={k}
                      label={k.toUpperCase()}
                      value={t[k]}
                      onChange={(v) =>
                        setDraft({
                          ...draft,
                          targets: {
                            ...draft.targets,
                            stages: draft.targets.stages.map((x, j) =>
                              i === j ? { ...x, [k]: v } : x,
                            ),
                          },
                        })
                      }
                    />
                  ))}
                </div>
              ))}
              <button
                onClick={() =>
                  setDraft({
                    ...draft,
                    targets: {
                      ...draft.targets,
                      stages: [...draft.targets.stages, target(50, 10)],
                    },
                  })
                }
              >
                增加阶段
              </button>
              <div className="exp-fields">
                <Numeric
                  label="最终日均销量目标"
                  value={draft.targets.finalTarget.dailyUnits}
                  onChange={(v) =>
                    setDraft({
                      ...draft,
                      targets: {
                        ...draft.targets,
                        finalTarget: {
                          ...draft.targets.finalTarget,
                          dailyUnits: v ?? 0,
                        },
                      },
                    })
                  }
                />
                <Numeric
                  label="最终 TACOS 上限 %"
                  value={draft.targets.finalTarget.tacosMax}
                  onChange={(v) =>
                    setDraft({
                      ...draft,
                      targets: {
                        ...draft.targets,
                        finalTarget: {
                          ...draft.targets.finalTarget,
                          tacosMax: v ?? 0,
                        },
                      },
                    })
                  }
                />
                <Numeric
                  label="TACOS 否决硬上限 %"
                  value={draft.targets.tacosHardLimit}
                  onChange={(v) =>
                    setDraft({
                      ...draft,
                      targets: { ...draft.targets, tacosHardLimit: v ?? 0 },
                    })
                  }
                />
                <Numeric
                  label="推荐 TACOS 最低 %"
                  value={draft.targets.preferredTacosMin}
                  onChange={(v) =>
                    setDraft({
                      ...draft,
                      targets: { ...draft.targets, preferredTacosMin: v ?? 0 },
                    })
                  }
                />
                <Numeric
                  label="推荐 TACOS 最高 %"
                  value={draft.targets.preferredTacosMax}
                  onChange={(v) =>
                    setDraft({
                      ...draft,
                      targets: { ...draft.targets, preferredTacosMax: v ?? 0 },
                    })
                  }
                />
              </div>
            </>
          )}
          {step === 5 && (
            <div className="exp-fields">
              <Numeric
                label="观察完整自然日（3–90）"
                value={draft.observationDays}
                onChange={(v) =>
                  setDraft({ ...draft, observationDays: v ?? 3 })
                }
              />
              <label>
                操作人
                <input
                  value={draft.operator}
                  onChange={(e) =>
                    setDraft({ ...draft, operator: e.target.value })
                  }
                />
              </label>
              <p>
                启动当天为 Day
                0，从次日开始观察；同步完成及实验页刷新后自动更新评估。
              </p>
            </div>
          )}
          {step === 6 && (
            <>
              <h3>{draft.name || "请填写名称"}</h3>
              <p>
                {kinds[draft.experimentType]} · {draft.changes.length} SKU ·
                基准 {draft.baselineStart} ～ {draft.baselineEnd} · 观察{" "}
                {draft.observationDays} 天
              </p>
              <p>
                保存后可查看草稿；启动时锁定基准。预算、暂停、恢复与价格的真实操作需通过现有控制页面执行；应用外改图、改页和促销需补记事件。
              </p>
              <button disabled={busy} onClick={() => void save()}>
                保存实验草稿
              </button>
            </>
          )}
          <footer className="exp-toolbar">
            <button disabled={step === 0} onClick={() => setStep(step - 1)}>
              上一步
            </button>
            <button disabled={step === 6} onClick={() => setStep(step + 1)}>
              下一步
            </button>
          </footer>
        </section>
      ) : (
        <>
          <section className="card">
            <div className="exp-toolbar">
              <input
                placeholder="搜索实验 / SKU"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <select
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              >
                <option value="">所有状态</option>
                {[
                  "draft",
                  "observing",
                  "success",
                  "hold",
                  "weak_success",
                  "rollback",
                  "stopped",
                  "completed",
                  "INSUFFICIENT_DATA",
                ].map((k) => (
                  <option key={k} value={k}>
                    {labels[k]}
                  </option>
                ))}
              </select>
              <select
                value={kindFilter}
                onChange={(e) => setKindFilter(e.target.value)}
              >
                <option value="">所有类型</option>
                {Object.entries(kinds).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </select>
              <label>
                创建日期起
                <input
                  type="date"
                  value={fromFilter}
                  onChange={(e) => setFromFilter(e.target.value)}
                />
              </label>
              <button
                disabled={busy}
                onClick={() =>
                  void reload().catch((e) => setMessage(String(e)))
                }
              >
                刷新并评估
              </button>
            </div>
            <div className="exp-table">
              <table>
                <thead>
                  <tr>
                    {[
                      "实验",
                      "阶段",
                      "状态 / 决策",
                      "观察",
                      "评分",
                      "日均销量 / 目标",
                      "TACOS",
                    ].map((s) => (
                      <th key={s}>{s}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((x) => (
                    <tr
                      key={x.id}
                      className={selected?.id === x.id ? "selected" : ""}
                    >
                      <td>
                        <button
                          onClick={() => {
                            setSelected(x);
                            setRestore(null);
                          }}
                        >
                          {x.input.name}
                        </button>
                        <small>
                          {kinds[x.input.experimentType]} ·{" "}
                          {x.input.changes.length} SKU
                        </small>
                      </td>
                      <td>
                        {x.stageIndex + 1}/{x.input.targets.stages.length}
                      </td>
                      <td>
                        {labels[x.status]}
                        <small>
                          {labels[x.evaluation?.decision || "DRAFT"]}
                        </small>
                      </td>
                      <td>
                        Day {x.evaluation?.completedDays || 0}/
                        {x.input.observationDays}
                      </td>
                      <td>{fmt(x.evaluation?.score)}</td>
                      <td>
                        {fmt(x.evaluation?.current?.dailyUnits)} /{" "}
                        {x.input.targets.stages[x.stageIndex].dailyUnits}
                      </td>
                      <td>{fmt(x.evaluation?.current?.tacos, "%")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {!filtered.length && (
              <p>
                还没有匹配的实验。从产品系列创建第一个实验，积累可复用的运营记录。
              </p>
            )}
          </section>
          {selected && (
            <>
              <section
                className={`card exp-decision ${e?.decision || "DRAFT"}`}
              >
                <div className="exp-toolbar">
                  <div>
                    <h2>{selected.input.name}</h2>
                    <strong>
                      {labels[e?.decision || "DRAFT"]} ·{" "}
                      {labels[selected.status]}
                    </strong>
                    <p>
                      Stage {selected.stageIndex + 1}/
                      {selected.input.targets.stages.length} · Day{" "}
                      {e?.completedDays || 0}/{selected.input.observationDays} ·
                      置信度 {labels[e?.confidence || "Low"]}
                    </p>
                  </div>
                  <div className="exp-score">
                    {fmt(e?.score)}
                    <small>/ 100</small>
                  </div>
                </div>
                <p>{e?.nextAction || "核对修改内容后启动，基准将永久锁定。"}</p>
                <div className="exp-toolbar">
                  {selected.status === "draft" && (
                    <><button disabled={busy} onClick={()=>{setDraft(selected.input);setEditingId(selected.id);setCreating(true);setContext(null);setStep(0);}}>编辑草稿</button>
                    <button disabled={busy} onClick={() => void run("start")}>
                      锁定基准并启动
                    </button></>
                  )}
                  {selected.status !== "draft" && (
                    <>
                      <button disabled={busy} onClick={()=>void run("confirm_execution")}>确认已在平台执行计划修改</button>
                      <button
                        disabled={busy}
                        onClick={() => void run("evaluate")}
                      >
                        重新评估
                      </button>
                      <button
                        disabled={
                          busy ||
                          !["observing", "hold", "weak_success"].includes(
                            selected.status,
                          )
                        }
                        onClick={() => void run("extend")}
                      >
                        延长观察 3 天
                      </button>
                      <button
                        disabled={busy || !e?.stageQualified}
                        onClick={() => void run("next_stage")}
                      >
                        进入下一阶段
                      </button>
                      <button
                        disabled={busy}
                        onClick={() => void run("retest")}
                      >
                        创建重测草稿
                      </button>
                      <button
                        disabled={
                          busy ||
                          e?.decision !== "SUCCESS" ||
                          e?.confidence === "Low"
                        }
                        onClick={() => void run("stable")}
                      >
                        设为稳定版本
                      </button>
                      <button
                        disabled={busy}
                        onClick={() => void run("complete")}
                      >
                        完成跟踪
                      </button>
                    </>
                  )}
                  <button disabled={busy} onClick={() => void run("stop")}>
                    停止跟踪
                  </button>
                  <button
                    onClick={() =>
                      void exportProductAnalysisJson(
                        `experiment-${selected.id}.json`,
                        selected,
                      )
                        .then((p) => setMessage(`已导出：${p}`))
                        .catch((e) => setMessage(String(e)))
                    }
                  >
                    导出实验 JSON
                  </button>
                </div>
              </section>
              {e?.current && (
                <>
                  <div className="exp-kpis">
                    <section className="card">
                      <small>当前日均销量</small>
                      <strong>{fmt(e.current.dailyUnits)} 件</strong>
                      <span>
                        阶段目标{" "}
                        {
                          selected.input.targets.stages[selected.stageIndex]
                            .dailyUnits
                        }{" "}
                        件；还差 {fmt(e.goals?.stageGap)}
                      </span>
                    </section>
                    <section className="card">
                      <small>
                        最终目标 {selected.input.targets.finalTarget.dailyUnits}{" "}
                        件/天
                      </small>
                      <strong>{fmt(e.goals?.finalProgress, "%")}</strong>
                      <progress
                        max={100}
                        value={Math.min(100, e.goals?.finalProgress ?? 0)}
                      />
                      <span>还差 {fmt(e.goals?.finalGap)} 件/天</span>
                    </section>
                    <section className="card">
                      <small>数据质量</small>
                      <strong>{labels[e.quality?.status] || "未知"}</strong>
                      <span>
                        部分 {e.quality?.partialDays ?? 0} 天 · 缺失{" "}
                        {e.quality?.missingDays ?? 0} 天
                      </span>
                      <span>
                        基准：{labels[e.baselineQuality?.status] || "未知"}
                      </span>
                    </section>
                    <section className="card">
                      <small>库存覆盖</small>
                      <strong>{fmt(e.stockDays, " 天")}</strong>
                      <span>低于 21 天禁止放量；未知库存需先核验</span>
                    </section>
                  </div>
                  <section className="card">
                    <h3>核心 KPI 对比 · 金额 RUB</h3>
                    <p>
                      花费、销售额和订单变化使用日均值比较；比率使用周期汇总分子分母重算。数据缺失显示
                      —。
                    </p>
                    <div className="exp-table">
                      <table>
                        <thead>
                          <tr>
                            <th>指标</th>
                            <th>锁定基准</th>
                            <th>当前</th>
                            <th>变化</th>
                            <th>阶段目标</th>
                            <th>状态</th>
                          </tr>
                        </thead>
                        <tbody>
                          {[
                            [
                              "dailyUnits",
                              "日均销量",
                              "salesGrowth",
                              "dailyUnits",
                              "min",
                            ],
                            [
                              "tacos",
                              "TACOS %",
                              "tacosChange",
                              "tacosMax",
                              "max",
                            ],
                            ["cvr", "CVR %", "cvrChange", "cvr", "min"],
                            ["cpa", "CPA ₽", "cpaChange", "cpa", "max"],
                            ["cpc", "CPC ₽", "cpcChange", "cpc", "max"],
                            ["acos", "ACOS %", "", "acos", "max"],
                            [
                              "dailySpend",
                              "日均广告费 ₽",
                              "spendGrowth",
                              "",
                              "max",
                            ],
                            [
                              "dailyRevenue",
                              "日均销售额 ₽",
                              "revenueGrowth",
                              "",
                              "min",
                            ],
                          ].map(([k, name, change, t, direction]) => {
                            const goal =
                              selected.input.targets.stages[
                                selected.stageIndex
                              ][t as keyof Target];
                            const value = e.current[k];
                            return (
                              <tr key={k}>
                                <td>{name}</td>
                                <td>{fmt(e.baseline[k])}</td>
                                <td>{fmt(value)}</td>
                                <td>
                                  {fmt(
                                    e.changes[change] == null
                                      ? null
                                      : e.changes[change]! *
                                          (change === "tacosChange" ? 1 : 100),
                                    change === "tacosChange" ? "pp" : "%",
                                  )}
                                </td>
                                <td>
                                  {goal == null
                                    ? "未设置"
                                    : `${direction === "min" ? "≥" : "≤"}${goal}`}
                                </td>
                                <td>
                                  {value == null
                                    ? "数据不足"
                                    : goal == null
                                      ? "观察"
                                      : (
                                            direction === "min"
                                              ? value >= goal
                                              : value <= goal
                                          )
                                        ? "达标"
                                        : "未达标"}
                                </td>
                              </tr>
                            );
                          })}
                        </tbody>
                      </table>
                    </div>
                    <div className="exp-kpis">
                      <div>
                        <small>放量弹性</small>
                        <strong>{fmt(e.changes.scaleElasticity)}</strong>
                        <span>仅正向加费时用于放量解释</span>
                      </div>
                      <div>
                        <small>边际 CPA ₽</small>
                        <strong>{fmt(e.marginal.cpa)}</strong>
                      </div>
                      <div>
                        <small>边际 ROAS</small>
                        <strong>{fmt(e.marginal.roas)}</strong>
                      </div>
                      <div>
                        <small>每新增 1000 ₽ 对应销量变化</small>
                        <strong>{fmt(e.marginal.unitsPer1000Rub)}</strong>
                      </div>
                    </div>
                    <p>
                      边际值为日均差额之比，不等于因果增量；自然销量不作推算。
                    </p>
                  </section>
                  <section className="card">
                    <h3>规则判断</h3>
                    <div className="exp-kpis">
                      {Object.entries(e.scoreComponents).map(([k, v]) => (
                        <div key={k}>
                          <small>
                            {
                              (
                                {
                                  sales: "销量 /35",
                                  tacos: "TACOS /25",
                                  cvr: "CVR /15",
                                  cpa: "CPA /10",
                                  cpc: "CPC /5",
                                  stability: "稳定性 /10",
                                } as Record<string, string>
                              )[k]
                            }
                          </small>
                          <strong>{fmt(v)}</strong>
                        </div>
                      ))}
                    </div>
                    {e.vetoes?.map((v) => (
                      <p className="exp-veto" key={v}>
                        {v}
                      </p>
                    ))}
                    <h4>事实</h4>
                    {e.facts?.map((f) => (
                      <p key={f}>{f}</p>
                    ))}
                    <h4>推断</h4>
                    {e.inference?.map((f) => (
                      <p key={f}>{f}</p>
                    ))}
                    <h4>建议</h4>
                    <p>{e.nextAction}</p>
                  </section>
                  <Trend value={e} />
                  <section className="card">
                    <h3>SKU 实验矩阵</h3>
                    <div className="exp-kpis">
                      {selected.input.changes.map((x) => {
                        const s = e.skuScores?.find((s) => s.sku === x.sku);
                        return (
                          <article className="exp-change" key={x.sku}>
                            <strong>
                              {products.find((p) => p.sku === x.sku)?.offerId ||
                                x.sku}
                            </strong>
                            <p>
                              {actions[x.action]} · {fmt(x.beforeBudget)} →{" "}
                              {fmt(x.afterBudget)} ₽ / 周
                            </p>
                            <p>
                              {x.beforeStatus || "未知"} →{" "}
                              {x.afterStatus || "未设置"}
                            </p>
                            <p>
                              CVR {fmt(s?.baseline?.cvr)} →{" "}
                              {fmt(s?.current?.cvr)} %
                            </p>
                            <p>
                              CPA {fmt(s?.baseline?.cpa)} →{" "}
                              {fmt(s?.current?.cpa)} ₽
                            </p>
                            <strong>
                              {labels[s?.decision || "INSUFFICIENT_DATA"]} ·{" "}
                              {fmt(s?.score)} 分
                            </strong>
                            <p>SKU 分项供诊断；阶段销量目标按系列考核。</p>
                          </article>
                        );
                      })}
                    </div>
                  </section>
                </>
              )}
              <section className="card">
                <div className="exp-toolbar">
                  <h3>AI 决策卡</h3>
                  <button
                    disabled={busy || selected.status === "draft"}
                    onClick={() => void run("ai")}
                  >
                    使用已配置 AI 分析
                  </button>
                </div>
                <p>
                  点击后将本实验数据及本店实验历史发送至连接设置中的 AI 服务。AI
                  不自动执行广告修改，规则否决优先。
                </p>
                {selected.ai ? (
                  <>
                    <small>
                      {selected.ai.evaluatedAt} ·
                      历史生成结果，数据刷新后可重新分析
                    </small>
                    <AiSections text={selected.ai.text}/>
                  </>
                ) : (
                  <p>尚未调用 AI。上方规则判断可离线使用。</p>
                )}
              </section>
              <section className="card">
                <h3>稳定版本 / 恢复清单</h3>
                <p>
                  恢复按钮生成逐项配置清单；平台变更仍需在现有广告与价格控制中核对执行。
                </p>
                {selected.stableVersions.map((s) => (
                  <button
                    disabled={busy}
                    key={s.id}
                    onClick={() => void run("rollback", { stableId: s.id })}
                  >
                    查看恢复配置：{s.name} #{s.id}
                  </button>
                ))}
                {!selected.stableVersions.length && (
                  <p>尚无关联稳定版本。实验成功后可保存。</p>
                )}
                {restore != null && (
                  <pre>{JSON.stringify(restore, null, 2)}</pre>
                )}
              </section>
              <section className="card">
                <h3>操作时间轴</h3>
                <p>
                  应用内成功的广告/价格操作会自动关联；应用外修改请补记。计划记录不表示平台已执行。
                </p>
                {selected.events.map((ev) => (
                  <article className="exp-event" key={ev.id}>
                    <strong>
                      {ev.time} · {ev.type} · {ev.sku}
                    </strong>
                    <p>
                      {ev.before} → {ev.after}
                    </p>
                    <small>
                      {ev.operator} · {ev.reason}
                    </small>
                  </article>
                ))}
                <div className="exp-fields">
                  <label>
                    SKU
                    <select
                      value={eventSku}
                      onChange={(e) => setEventSku(e.target.value)}
                    >
                      <option value="">整个系列</option>
                      {selected.input.changes.map((c) => (
                        <option key={c.sku}>{c.sku}</option>
                      ))}
                    </select>
                  </label>
                  <label>
                    修改前
                    <input
                      value={eventBefore}
                      onChange={(e) => setEventBefore(e.target.value)}
                    />
                  </label>
                  <label>
                    修改后
                    <input
                      value={eventAfter}
                      onChange={(e) => setEventAfter(e.target.value)}
                    />
                  </label>
                  <label>
                    原因 / 修改内容
                    <input
                      value={eventReason}
                      onChange={(e) => setEventReason(e.target.value)}
                    />
                  </label>
                  <button
                    disabled={busy || !eventReason}
                    onClick={() =>
                      void run("event", {
                        sku: eventSku,
                        before: eventBefore,
                        after: eventAfter,
                        reason: eventReason,
                      })
                    }
                  >
                    补记已执行操作
                  </button>
                </div>
              </section>
            </>
          )}
        </>
      )}
    </div>
  );
}
