import { useEffect, useRef, useState } from "react";
import "./analytics.css";
import "./listing.css";
import * as echarts from "./charts";
import {
  AlertTriangle,
  CalendarDays,
  CheckCircle2,
  Clock3,
  Database,
  MapPin,
  Megaphone,
  PackageSearch,
  Plus,
  RefreshCw,
  Save,
  Search,
  Trash2,
  TrendingUp,
  Truck,
} from "lucide-react";
import {
  addCompetitor,
  aiAnalysis,
  analyticsDetail,
  bookSupplyTimeslot,
  supplyClusterPlans,
  saveSupplyClusterPlan,
  businessReport,
  crossBorderReport,
  calculateListingPrice,
  createListingDraft,
  saveListingDraft,
  retryListingJob,
  collectListingReference,
  openListingBrowser,
  importListingHtml,
  refreshListingCategories,
  searchListingCategories,
  competitors,
  competitorAlertSettings,
  competitorLatestRun,
  competitorCollectionProgress,
  dataCoverage,
  deleteCompetitorDemoData,
  exportDataset,
  exportApiBundle,
  exportWbApiBundle,
  fbsOrders,
  financeBreakdown,
  importProductCostsCsv,
  importApiBundle,
  importWbApiBundle,
  listingRows,
  listingJobs,
  listingSettings,
  missingCostRows,
  notifyShipment,
  pruneCache,
  rerunCompetitorsCollection,
  retryFailedCompetitorsCollection,
  startCompetitorsCollection,
  startCompetitorCollectionTask,
  stopCompetitorsCollection,
  stopCompetitorCollectionTask,
  setCompetitorManualMetrics,
  seedCompetitorDemoData,
  removeCompetitor,
  saveFbsThreshold,
  saveListingSettings,
  saveProductCost,
  saveCompetitorAlertSettings,
  saveWarehouseMapping,
  saveWbCost,
  saveWbSettings,
  sendFeishuWeekly,
  sendWbWeekly,
  shipmentTracking,
  shipmentSkuOptions,
  saveShipmentSkuAllocations,
  shipmentSettlement,
  settleShipment,
  supplyOrders,
  supplyTimeslots,
  syncFbsOrders,
  syncFeishuProducts,
  syncFeishuShipments,
  syncFinance,
  syncAllData,
  syncListingCosts,
  syncLogs,
  syncPerformance,
  syncSeller,
  syncWb,
  testFeishu,
  testWbFeishu,
  warehouseMappings,
  wbCosts,
  wbDaily,
  wbOrders,
  wbAds,
  wbWarehouses,
  wbStocks,
  wbSettings,
} from "./bridge";
import type {
  AnalyticsDetail,
  BusinessReport,
  CrossBorderReport,
  CompetitorAlertSettings,
  CompetitorRow,
  CompetitorRunSummary,
  CompetitorCollectionProgress,
  DataCoverageRow,
  DateRange,
  FbsOrderRow,
  FinanceBreakdownRow,
  ListingRow,
  ListingJob,
  ListingCategory,
  ListingDraftInput,
  ListingPriceBreakdown,
  ListingPriceInput,
  ListingSettings,
  MissingCostRow,
  ShipmentTracking,
  ShipmentSkuAllocation,
  ShipmentSkuOption,
  ShipmentSettlementItem,
  SupplyOrder,
  SupplyClusterPlan,
  SupplyTimeslot,
  SyncLog,
  WarehouseMapping,
  WbCost,
  WbDaily,
  WbOrderRow,
  WbAdRow,
  WbWarehouseRow,
  WbStockRow,
  WbSettings,
} from "./types";

const money = (v: number, currency: string) =>
  `${currency === "CNY" ? "¥" : "₽"}${v.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
const reportCache = new Map<string, Promise<BusinessReport>>(),
  detailCache = new Map<string, Promise<AnalyticsDetail>>();
const cachedReport = (range: DateRange) => {
  const key = `${range.from}|${range.to}`;
  if (!reportCache.has(key)) reportCache.set(key, businessReport(range));
  return reportCache.get(key)!;
};
const cachedDetail = (range: DateRange) => {
  const key = `${range.from}|${range.to}`;
  if (!detailCache.has(key)) detailCache.set(key, analyticsDetail(range));
  return detailCache.get(key)!;
};

function MissingCostEditor({
  row,
  close,
  saved,
}: {
  row: MissingCostRow;
  close: () => void;
  saved: () => void;
}) {
  const value = (v: number | null) => (v == null ? "" : String(v));
  const [form, setForm] = useState({
    unitCost: value(row.unitCost),
    firstMileCost: value(row.firstMileCost),
    lengthCm: value(row.lengthCm),
    widthCm: value(row.widthCm),
    heightCm: value(row.heightCm),
    weightKg: value(row.weightKg),
    note: row.note || "",
  });
  const [busy, setBusy] = useState(false);
  const numeric = (v: string) => (v.trim() === "" ? null : Number(v));
  const field = (key: keyof typeof form, label: string, required = false) => (
    <label>
      {label}
      {required && <b className="missing-cost">（缺失）</b>}
      <input
        type={key === "note" ? "text" : "number"}
        min={key === "note" ? undefined : "0"}
        step="any"
        value={form[key]}
        onChange={(e) => setForm({ ...form, [key]: e.target.value })}
      />
    </label>
  );
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div className="cost-modal">
        <button className="modal-close" onClick={close}>
          ×
        </button>
        <h2>
          {row.offerId || "未命名货号"} / {row.sku}
        </h2>
        <p>{row.productName || "补充成本后，月度盈亏将自动重新核算。"}</p>
        <div className="cost-fields">
          {field("unitCost", "采购成本（CNY/件）", row.missingPurchase)}
          {field("firstMileCost", "头程成本（RUB/件）", row.missingFirstMile)}
          {field("lengthCm", "长度（cm）", row.missingDimensions)}
          {field("widthCm", "宽度（cm）", row.missingDimensions)}
          {field("heightCm", "高度（cm）", row.missingDimensions)}
          {field("weightKg", "重量（kg）", row.missingWeight)}
          <label className="wide">
            备注
            <input
              value={form.note}
              onChange={(e) => setForm({ ...form, note: e.target.value })}
            />
          </label>
        </div>
        <div className="modal-actions">
          <button
            className="dark-button"
            disabled={busy}
            onClick={async () => {
              const numbers = [
                form.unitCost,
                form.firstMileCost,
                form.lengthCm,
                form.widthCm,
                form.heightCm,
                form.weightKg,
              ];
              if (
                numbers.some(
                  (v) =>
                    v.trim() !== "" &&
                    (!Number.isFinite(Number(v)) || Number(v) < 0),
                )
              ) {
                window.alert("成本、尺寸和重量必须是大于或等于 0 的数字。");
                return;
              }
              setBusy(true);
              try {
                await saveProductCost({
                  sku: row.sku,
                  unitCost: numeric(form.unitCost),
                  firstMileCost: numeric(form.firstMileCost),
                  lengthCm: numeric(form.lengthCm),
                  widthCm: numeric(form.widthCm),
                  heightCm: numeric(form.heightCm),
                  weightKg: numeric(form.weightKg),
                  note: form.note,
                });
                saved();
              } catch (e) {
                window.alert(`保存失败：${String(e)}`);
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "保存中…" : "保存并重新核算"}
          </button>
          <button className="outline-button" onClick={close}>
            取消
          </button>
        </div>
      </div>
    </div>
  );
}
export const clearReportCache = () => {
  reportCache.clear();
  detailCache.clear();
};
window.addEventListener("report-settings-changed", clearReportCache);

export function ListingPage() {
  const empty: ListingSettings = {
    ledgerPath: "",
    ledgerShopName: "",
    toolExecutable: "",
    toolDataDir: "",
  };
  const [form, setForm] = useState<ListingSettings>(empty),
    [priceForm, setPriceForm] = useState<ListingPriceInput>({
      purchaseCost: 0,
      labelFee: 0,
      targetRoiPercent: 30,
      weightKg: 0,
      salesCommissionPercent: 15,
      salesCommissionDiscountPercent: 0,
      advertisingPercent: 15,
      cargoLossPercent: 10,
      minimumSalePrice: 0,
    }),
    [priceResult, setPriceResult] = useState<ListingPriceBreakdown | null>(
      null,
    ),
    [rows, setRows] = useState<ListingRow[]>([]),
    [jobs, setJobs] = useState<ListingJob[]>([]),
    [draft, setDraft] = useState<ListingDraftInput | null>(null),
    [attributesText, setAttributesText] = useState("[]"),
    [complexAttributesText, setComplexAttributesText] = useState("[]"),
    [imagesText, setImagesText] = useState(""),
    [reference, setReference] = useState(""),
    [listingHtmlPath, setListingHtmlPath] = useState(""),
    [categoryQuery, setCategoryQuery] = useState(""),
    [categoryRows, setCategoryRows] = useState<ListingCategory[]>([]),
    [query, setQuery] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const editJob = (job: ListingJob) => {
    const p = job.payload || {};
    const numberValue = (key: string) => Number(p[key] || 0);
    setDraft({
      id: job.id,
      offerId: String(p.offer_id || job.offerId || ""),
      title: String(p.title || job.title || ""),
      categoryId: String(p.category_id || job.categoryId || ""),
      categoryDisplay: String(p.category_display || job.categoryDisplay || ""),
      typeId: String(p.type_id || ""),
      price: String(p.price || ""),
      weight: numberValue("weight"),
      depth: numberValue("depth"),
      width: numberValue("width"),
      height: numberValue("height"),
      description: String(p.description || ""),
      images: [],
      attributes: [],
      complexAttributes: [],
    });
    setImagesText(
      Array.isArray(p.images) ? p.images.map(String).join("\n") : "",
    );
    setAttributesText(
      JSON.stringify(Array.isArray(p.attributes) ? p.attributes : [], null, 2),
    );
    setComplexAttributesText(
      JSON.stringify(
        Array.isArray(p.complex_attributes) ? p.complex_attributes : [],
        null,
        2,
      ),
    );
  };
  const load = async () => {
    setBusy(true);
    setMessage("");
    try {
      const settings = await listingSettings();
      setForm(settings);
      setRows(await listingRows(query));
      setJobs(await listingJobs());
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void load();
  }, [query]);
  const save = async () => {
    setBusy(true);
    try {
      await saveListingSettings(form);
      setMessage("上品工具与产品台账配置已保存。");
      setRows(await listingRows(query));
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  const sync = async () => {
    setBusy(true);
    try {
      const count = await syncListingCosts();
      setMessage(`成本、长宽高和重量已统一匹配到 ${count} 个 SKU。`);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">CROSS-BORDER LISTING</span>
          <h1>跨境上品</h1>
          <p>
            RFBS 上品源码工作台：产品台账、原版 ROI 核价与后续上架任务均在 ERP
            内完成
          </p>
        </div>
        <div className="competitor-actions">
          <button className="outline-button" disabled={busy} onClick={sync}>
            <RefreshCw size={15} />
            立即同步成本
          </button>
        </div>
      </header>
      <section className="card listing-config">
        <div className="card-title">参考商品与上品草稿</div>
        <div className="listing-fields">
          <label>
            Ozon 商品链接或 Артикул
            <input
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              placeholder="支持链接、纯数字或 Артикул: 2379505289"
            />
          </label>
        </div>
        <button
          className="dark-button"
          disabled={busy || !reference.trim()}
          onClick={async () => {
            setBusy(true);
            try {
              const id = await createListingDraft(reference);
              setMessage(
                `已创建上品草稿 #${id}，下一阶段将从参考商品采集标题、图片和属性。`,
              );
              setReference("");
              setJobs(await listingJobs());
            } catch (e) {
              setMessage(String(e));
            } finally {
              setBusy(false);
            }
          }}
        >
          <Plus size={15} />
          创建本地草稿
        </button>
        {!!jobs.length && (
          <div className="table-card">
            <table>
              <thead>
                <tr>
                  <th>任务</th>
                  <th>Артикул / 货号</th>
                  <th>类目</th>
                  <th>阶段 / 状态</th>
                  <th>更新时间</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {jobs.slice(0, 10).map((job) => (
                  <tr key={job.id}>
                    <td>#{job.id}</td>
                    <td>
                      <b>{job.article}</b>
                      <small>{job.offerId || job.sourceUrl}</small>
                    </td>
                    <td>
                      {job.categoryDisplay || "待匹配"}
                      <small>{job.categoryId}</small>
                    </td>
                    <td>
                      {
                        ["待采集", "参考信息", "类目完成", "属性完成"][
                          Math.min(job.stage, 3)
                        ]
                      }{" "}
                      / {job.status}
                      <small>{job.error}</small>
                    </td>
                    <td>{job.updatedAt}</td>
                    <td>
                      <button
                        className="outline-button"
                        onClick={() => editJob(job)}
                      >
                        编辑
                      </button>
                      <button
                        className="outline-button"
                        disabled={busy || job.status === "collecting"}
                        onClick={async () => {
                          setBusy(true);
                          setMessage(`正在后台采集任务 #${job.id}…`);
                          try {
                            await collectListingReference(job.id);
                            setMessage(
                              `任务 #${job.id} 已采集标题、描述、图片和页面属性。`,
                            );
                          } catch (e) {
                            setMessage(String(e));
                          } finally {
                            const next = await listingJobs();
                            setJobs(next);
                            const current = next.find((v) => v.id === job.id);
                            if (current && draft?.id === job.id)
                              editJob(current);
                            setBusy(false);
                          }
                        }}
                      >
                        采集
                      </button>
                      {job.error && (
                        <button
                          className="outline-button"
                          onClick={async () => {
                            await retryListingJob(job.id);
                            setJobs(await listingJobs());
                          }}
                        >
                          重试
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {draft && (
          <div className="listing-draft-editor">
            <div className="card-title">编辑草稿 #{draft.id}</div>
            <div className="migration-note">
              <AlertTriangle />
              <div>
                <h3>浏览器验证采集</h3>
                <p>
                  直连遇到重定向或验证时，打开浏览器完成验证，等待标题和图库显示完整，然后按
                  Ctrl+S 保存“网页，仅 HTML”。应用只读取你选择的 HTML，不读取
                  Cookie 或浏览器登录信息。
                </p>
                <div className="listing-fields">
                  <label>
                    验证后 HTML 完整路径
                    <input
                      value={listingHtmlPath}
                      onChange={(e) => setListingHtmlPath(e.target.value)}
                      placeholder="例如 D:\\下载\\Ozon商品.html"
                    />
                  </label>
                </div>
                <div className="competitor-actions">
                  <button
                    className="outline-button"
                    onClick={async () => {
                      try {
                        await openListingBrowser(draft.id);
                        setMessage(
                          "已打开商品页；完成验证并等待图库加载后，请保存 HTML。",
                        );
                      } catch (e) {
                        setMessage(String(e));
                      }
                    }}
                  >
                    打开 Edge/浏览器
                  </button>
                  <button
                    className="dark-button"
                    disabled={busy || !listingHtmlPath.trim()}
                    onClick={async () => {
                      setBusy(true);
                      try {
                        const count = await importListingHtml(
                          draft.id,
                          listingHtmlPath,
                        );
                        const next = await listingJobs();
                        setJobs(next);
                        const current = next.find((v) => v.id === draft.id);
                        if (current) editJob(current);
                        setMessage(
                          `验证页采集成功：已保存标题、描述、参数和 ${count} 张商品图片。`,
                        );
                        setListingHtmlPath("");
                      } catch (e) {
                        setMessage(String(e));
                      } finally {
                        setBusy(false);
                      }
                    }}
                  >
                    导入验证页并继续
                  </button>
                </div>
              </div>
            </div>
            <div className="listing-fields">
              <label>
                实时类目搜索
                <input
                  value={categoryQuery}
                  onChange={async (e) => {
                    const value = e.target.value;
                    setCategoryQuery(value);
                    try {
                      setCategoryRows(await searchListingCategories(value, 60));
                    } catch (error) {
                      setCategoryRows([]);
                      if (value.trim()) setMessage(String(error));
                    }
                  }}
                  placeholder="输入中文或俄文商品类型"
                />
              </label>
              <div className="competitor-actions">
                <button
                  className="outline-button"
                  disabled={busy}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      const count = await refreshListingCategories();
                      setCategoryRows(
                        await searchListingCategories(
                          categoryQuery || draft.title,
                          60,
                        ),
                      );
                      setMessage(`已缓存 ${count} 个 Ozon 实时末级类目。`);
                    } catch (e) {
                      setMessage(String(e));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  <RefreshCw size={14} />
                  刷新 Ozon 类目
                </button>
              </div>
            </div>
            {!!categoryRows.length && (
              <div className="table-card">
                <table>
                  <thead>
                    <tr>
                      <th>候选类目</th>
                      <th>类目 ID</th>
                      <th>type ID</th>
                      <th></th>
                    </tr>
                  </thead>
                  <tbody>
                    {categoryRows.slice(0, 20).map((row) => (
                      <tr key={`${row.descriptionCategoryId}-${row.typeId}`}>
                        <td>{row.display}</td>
                        <td>{row.descriptionCategoryId}</td>
                        <td>{row.typeId}</td>
                        <td>
                          <button
                            className="outline-button"
                            onClick={() => {
                              setDraft({
                                ...draft,
                                categoryId: String(row.descriptionCategoryId),
                                categoryDisplay: row.display,
                                typeId: String(row.typeId),
                              });
                              setCategoryRows([]);
                              setMessage(`已选择实时类目：${row.display}`);
                            }}
                          >
                            选择
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <div className="listing-fields">
              {(
                [
                  ["offerId", "货号 offer_id"],
                  ["title", "俄文标题"],
                  ["categoryId", "description_category_id"],
                  ["categoryDisplay", "类目路径"],
                  ["typeId", "type_id"],
                  ["price", "售价 RUB"],
                ] as Array<[keyof ListingDraftInput, string]>
              ).map(([key, label]) => (
                <label key={key}>
                  {label}
                  <input
                    value={String(draft[key])}
                    onChange={(e) =>
                      setDraft({ ...draft, [key]: e.target.value })
                    }
                  />
                </label>
              ))}
              {(
                [
                  ["weight", "重量 g"],
                  ["depth", "深度 mm"],
                  ["width", "宽度 mm"],
                  ["height", "高度 mm"],
                ] as Array<[keyof ListingDraftInput, string]>
              ).map(([key, label]) => (
                <label key={key}>
                  {label}
                  <input
                    type="number"
                    min="0"
                    value={Number(draft[key])}
                    onChange={(e) =>
                      setDraft({ ...draft, [key]: Number(e.target.value) })
                    }
                  />
                </label>
              ))}
            </div>
            <label>
              俄文描述
              <textarea
                value={draft.description}
                onChange={(e) =>
                  setDraft({ ...draft, description: e.target.value })
                }
              />
            </label>
            <label>
              图片地址（每行一个）
              <textarea
                value={imagesText}
                onChange={(e) => setImagesText(e.target.value)}
              />
            </label>
            <div className="listing-fields">
              <label>
                attributes JSON
                <textarea
                  value={attributesText}
                  onChange={(e) => setAttributesText(e.target.value)}
                />
              </label>
              <label>
                complex_attributes JSON
                <textarea
                  value={complexAttributesText}
                  onChange={(e) => setComplexAttributesText(e.target.value)}
                />
              </label>
            </div>
            <div className="competitor-actions">
              <button
                className="dark-button"
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    const attributes = JSON.parse(attributesText),
                      complexAttributes = JSON.parse(complexAttributesText);
                    const stage = await saveListingDraft({
                      ...draft,
                      images: imagesText
                        .split(/\r?\n/)
                        .map((v) => v.trim())
                        .filter(Boolean),
                      attributes,
                      complexAttributes,
                    });
                    setMessage(`草稿 #${draft.id} 已保存，当前阶段 ${stage}。`);
                    const next = await listingJobs();
                    setJobs(next);
                    const current = next.find((v) => v.id === draft.id);
                    if (current) editJob(current);
                  } catch (e) {
                    setMessage(`保存失败：${String(e)}`);
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                <Save size={14} />
                保存并校验阶段
              </button>
              <button className="outline-button" onClick={() => setDraft(null)}>
                关闭
              </button>
            </div>
          </div>
        )}
      </section>
      <section className="card listing-config">
        <div className="card-title">台账配置</div>
        <div className="listing-fields">
          <label>
            产品台账.xlsx
            <input
              value={form.ledgerPath}
              onChange={(e) => setForm({ ...form, ledgerPath: e.target.value })}
              placeholder="完整文件路径"
            />
          </label>
          <label>
            台账内上品店铺
            <input
              value={form.ledgerShopName}
              onChange={(e) =>
                setForm({ ...form, ledgerShopName: e.target.value })
              }
              placeholder="必须指定，避免店铺成本混合"
            />
          </label>
        </div>
        <button className="outline-button" onClick={save} disabled={busy}>
          <Save size={14} />
          保存配置并读取
        </button>
        {message && <p className="listing-message">{message}</p>}
      </section>
      <section className="card listing-config">
        <div className="card-title">RFBS 原版 ROI 核价</div>
        <div className="listing-fields">
          {(
            [
              ["purchaseCost", "采购成本 CNY"],
              ["labelFee", "贴单费 CNY"],
              ["weightKg", "实际重量 kg"],
              ["targetRoiPercent", "目标 ROI %"],
              ["salesCommissionPercent", "销售佣金 %"],
              ["salesCommissionDiscountPercent", "佣金减免 %"],
              ["advertisingPercent", "广告费率 %"],
              ["cargoLossPercent", "货损率 %"],
              ["minimumSalePrice", "最低售价 CNY"],
            ] as Array<[keyof ListingPriceInput, string]>
          ).map(([key, label]) => (
            <label key={key}>
              {label}
              <input
                type="number"
                step="0.01"
                value={priceForm[key]}
                onChange={(e) =>
                  setPriceForm({ ...priceForm, [key]: Number(e.target.value) })
                }
              />
            </label>
          ))}
        </div>
        <button
          className="dark-button"
          disabled={busy}
          onClick={async () => {
            try {
              setPriceResult(await calculateListingPrice(priceForm));
              setMessage(
                "已按 RFBS 源码中的分段运费、2% 物流佣金、广告费和货损公式完成核价。",
              );
            } catch (e) {
              setMessage(String(e));
            }
          }}
        >
          <PackageSearch size={15} />
          计算建议售价
        </button>
        {priceResult && (
          <div className="report-hero listing-price-result">
            <div>
              <span>建议售价</span>
              <strong>¥{priceResult.price.toFixed(2)}</strong>
            </div>
            <div>
              <span>分段运费</span>
              <strong>¥{priceResult.shipping.toFixed(2)}</strong>
            </div>
            <div>
              <span>预计利润</span>
              <strong>¥{priceResult.profit.toFixed(2)}</strong>
            </div>
            <div>
              <span>实际 ROI</span>
              <strong>{priceResult.roiPercent.toFixed(2)}%</strong>
            </div>
          </div>
        )}
      </section>
      <div className="orders-tools listing-tools">
        <label className="search">
          <Search size={15} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索店铺、货号或商品 ID"
          />
        </label>
        <span>当前显示 {rows.length} 条</span>
      </div>
      <section className="card table-card">
        <table>
          <thead>
            <tr>
              <th>上品店铺 / 平台</th>
              <th>货号</th>
              <th>Ozon 商品 ID</th>
              <th>采购成本</th>
              <th>包装重量</th>
              <th>包装长×宽×高</th>
              <th>模式 / 核价</th>
              <th>售价 / 利润</th>
              <th>类目 / 导入任务</th>
              <th>状态 / 更新时间</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={`${row.offerId}-${i}`}>
                <td>
                  <b>{row.shopName}</b>
                  <small>{row.platform || "Ozon"}</small>
                </td>
                <td>
                  <b>{row.offerId}</b>
                </td>
                <td>{row.productId || "—"}</td>
                <td>
                  {row.unitCostCny == null
                    ? "—"
                    : `¥${row.unitCostCny.toFixed(2)}`}
                </td>
                <td>
                  {row.weightKg == null ? "—" : `${row.weightKg.toFixed(3)} kg`}
                </td>
                <td>
                  {[row.lengthCm, row.widthCm, row.heightCm].some(
                    (x) => x == null,
                  )
                    ? "—"
                    : `${row.lengthCm} × ${row.widthCm} × ${row.heightCm} cm`}
                </td>
                <td>
                  {row.listingMode || "—"}
                  <small>{row.pricingMode || "—"}</small>
                </td>
                <td>
                  {row.price == null ? "—" : row.price.toFixed(2)}
                  <small>
                    {row.profit == null
                      ? "—"
                      : `利润 ${row.profit.toFixed(2)} · ROI ${row.roiPercent?.toFixed(1) ?? "—"}%`}
                  </small>
                </td>
                <td>
                  {row.category || "—"}
                  <small>{row.importTaskId || "—"}</small>
                </td>
                <td>
                  <b>{row.status || "—"}</b>
                  <small>{row.updatedAt}</small>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!rows.length && !busy && (
          <div className="empty">
            配置产品台账路径和上品店铺后显示真实台账记录。
          </div>
        )}
      </section>
    </>
  );
}

export function FbsPage({ currency }: { currency: string }) {
  const [tab, setTab] = useState<"orders" | "mapping">("orders"),
    [rows, setRows] = useState<FbsOrderRow[]>([]),
    [mappings, setMappings] = useState<WarehouseMapping[]>([]),
    [query, setQuery] = useState(""),
    [hours, setHours] = useState(24),
    [warning, setWarning] = useState(4),
    [loading, setLoading] = useState(false);
  const load = async () => {
    setLoading(true);
    try {
      setRows(await fbsOrders(query));
      setMappings(await warehouseMappings());
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => {
    void load();
  }, [query]);
  const overdue = rows.filter((x) => x.alertLevel === "overdue").length,
    due = rows.filter((x) => x.alertLevel === "warning").length,
    pending = rows.filter((x) => x.alertLevel === "pending").length;
  const saveThreshold = async () => {
    await saveFbsThreshold(hours, warning);
    await load();
  };
  const syncApi = async () => {
    setLoading(true);
    try {
      const to = new Date(),
        from = new Date();
      from.setDate(to.getDate() - 29);
      await syncFbsOrders({
        from: from.toISOString().slice(0, 10),
        to: to.toISOString().slice(0, 10),
      });
      await load();
    } finally {
      setLoading(false);
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">FBS ORDER CONTROL</span>
          <h1>FBS 订单管理</h1>
          <p>独立获取和管理 FBS 订单、发货时效、超时预警与基础配送费估算</p>
        </div>
        <button className="dark-button" onClick={syncApi}>
          <Truck size={16} />
          {loading ? "同步中" : "从 Ozon 同步 FBS"}
        </button>
      </header>
      <div className="mini-stats">
        <div className={overdue ? "warn" : ""}>
          <AlertTriangle />
          <span>
            已经超时<strong>{overdue}</strong>
          </span>
        </div>
        <div className={due ? "warn" : ""}>
          <Clock3 />
          <span>
            即将到期<strong>{due}</strong>
          </span>
        </div>
        <div>
          <Truck />
          <span>
            待发货<strong>{pending}</strong>
          </span>
        </div>
        <div>
          <CheckCircle2 />
          <span>
            已进入发货<strong>{rows.length - overdue - due - pending}</strong>
          </span>
        </div>
      </div>
      <div className="fbs-controls">
        <div className="tabs">
          <button
            className={tab === "orders" ? "selected" : ""}
            onClick={() => setTab("orders")}
          >
            FBS 订单
          </button>
          <button
            className={tab === "mapping" ? "selected" : ""}
            onClick={() => setTab("mapping")}
          >
            仓库集群映射
          </button>
        </div>
        <div className="thresholds">
          <label>
            发货时效
            <input
              type="number"
              value={hours}
              onChange={(e) => setHours(Number(e.target.value))}
            />
            小时
          </label>
          <label>
            提前预警
            <input
              type="number"
              value={warning}
              onChange={(e) => setWarning(Number(e.target.value))}
            />
            小时
          </label>
          <button className="outline-button" onClick={saveThreshold}>
            <Save size={14} />
            保存阈值
          </button>
        </div>
        <label className="search">
          <Search size={15} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="订单、货号或 SKU"
          />
        </label>
      </div>
      {tab === "orders" ? (
        <section className="card table-card">
          <table>
            <thead>
              <tr>
                <th>订单 / 货号</th>
                <th>货号 / SKU</th>
                <th>下单时间</th>
                <th>状态</th>
                <th>发货截止</th>
                <th>运输 / 配送</th>
                <th>预估基础配送</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row, i) => (
                <tr key={`${row.postingNumber}-${i}`}>
                  <td>
                    <b>{row.postingNumber}</b>
                    <small>{row.offerId || row.sku}</small>
                  </td>
                  <td>
                    <b className="product">{row.offerId || "—"}</b>
                    <small>{row.sku}</small>
                  </td>
                  <td>{row.orderedAt}</td>
                  <td>
                    <span className={`alert-tag ${row.alertLevel}`}>
                      {row.alertLevel === "overdue"
                        ? "已超时"
                        : row.alertLevel === "warning"
                          ? "即将到期"
                          : row.alertLevel === "pending"
                            ? "待发货"
                            : "已发货"}
                    </span>
                    <small>{row.status}</small>
                  </td>
                  <td>{row.deadline || "—"}</td>
                  <td>
                    <b>{row.origin || "—"}</b>
                    <small>{row.destination || "—"}</small>
                  </td>
                  <td>
                    <b>
                      {row.estimatedDelivery == null
                        ? "—"
                        : money(row.estimatedDelivery, currency)}
                    </b>
                    <small>{row.estimateBasis}</small>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!rows.length && (
            <div className="empty">
              当前店铺尚无 FBS 缓存订单；同步 FBS 后会自动显示。
            </div>
          )}
        </section>
      ) : (
        <MappingTable rows={mappings} reload={load} />
      )}
    </>
  );
}

function MappingTable({
  rows,
  reload,
}: {
  rows: WarehouseMapping[];
  reload: () => void;
}) {
  const [draft, setDraft] = useState<Record<string, string>>({});
  const save = async (row: WarehouseMapping) => {
    await saveWarehouseMapping(
      row.warehouseName,
      draft[row.warehouseName] ?? row.clusterName,
    );
    await reload();
  };
  return (
    <section className="card mapping-panel">
      <div className="mapping-intro">
        <MapPin />
        <div>
          <h3>仓库所属集群</h3>
          <p>
            把未标注集群的仓库映射到配送表集群。订单会优先精确计算；没有直接线路时按历史订单集群权重估算。
          </p>
        </div>
      </div>
      <table>
        <thead>
          <tr>
            <th>仓库 / 地址</th>
            <th>历史订单数</th>
            <th>所属配送集群</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.warehouseName}>
              <td>
                <b>{row.warehouseName}</b>
              </td>
              <td>{row.orderCount}</td>
              <td>
                <input
                  className="mapping-input"
                  value={draft[row.warehouseName] ?? row.clusterName}
                  onChange={(e) =>
                    setDraft({ ...draft, [row.warehouseName]: e.target.value })
                  }
                  placeholder="输入配送表中的集群名称"
                />
              </td>
              <td>
                <button className="outline-button" onClick={() => save(row)}>
                  <Save size={14} />
                  保存
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {!rows.length && (
        <div className="empty">订单缓存中没有可映射的仓库。</div>
      )}
    </section>
  );
}

export function CompetitorsPage() {
  const [rows, setRows] = useState<CompetitorRow[]>([]),
    [url, setUrl] = useState(""),
    [selected, setSelected] = useState<number[]>([]),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [manualSales, setManualSales] = useState<
      Record<number, { daily: string; weekly: string; monthly: string }>
    >({}),
    [latestRun, setLatestRun] = useState<CompetitorRunSummary | null>(null),
    [alertSettings, setAlertSettings] =
      useState<CompetitorAlertSettings | null>(null),
    [savingAlerts, setSavingAlerts] = useState(false),
    [salesPeriod, setSalesPeriod] = useState<"daily" | "weekly" | "monthly">("daily"),
    [collection, setCollection] = useState<CompetitorCollectionProgress | null>(
      null,
    );
  const collectionWasRunning = useRef(false);
  const load = async () => {
    const [data, run, settings] = await Promise.all([
      competitors(),
      competitorLatestRun(),
      competitorAlertSettings(),
    ]);
    setRows(data);
    setLatestRun(run);
    setAlertSettings(settings);
    setSelected((old) => (old.length ? old : data.map((x) => x.id)));
  };
  useEffect(() => {
    void load().catch((e) => setError(`无法读取竞品任务：${String(e)}`));
  }, []);
  useEffect(() => {
    const poll = async () => {
      try {
        const next = await competitorCollectionProgress();
        setCollection(next);
        if (collectionWasRunning.current && !next.running) await load();
        collectionWasRunning.current = next.running;
      } catch (e) {
        setError(`无法读取采集进度：${String(e)}`);
      }
    };
    void poll();
    const timer = window.setInterval(poll, 700);
    return () => window.clearInterval(timer);
  }, []);
  const add = async () => {
    setBusy(true);
    setError("");
    try {
      await addCompetitor(url);
      setUrl("");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const startOne = async (id: number) => {
    setError("");
    try {
      await startCompetitorCollectionTask(id);
      setCollection(await competitorCollectionProgress());
    } catch (e) {
      setError(String(e));
    }
  };
  const refreshAll = async () => {
    if (!rows.length) return;
    setError("");
    try {
      await startCompetitorsCollection();
      setCollection(await competitorCollectionProgress());
    } catch (e) {
      setError(String(e));
    }
  };
  const rerunBatch = async (failedOnly = false) => {
    setError("");
    try {
      if (failedOnly) await retryFailedCompetitorsCollection();
      else await rerunCompetitorsCollection();
      setCollection(await competitorCollectionProgress());
    } catch (e) {
      setError(String(e));
    }
  };
  useEffect(() => {
    const el = document.getElementById("competitor-chart");
    if (!el) return;
    const chart = echarts.init(el);
    const chosen = rows.filter((x) => selected.includes(x.id));
    const dates = [
      ...new Set(
        chosen.flatMap((x) =>
          x.snapshots.map((s) => s.capturedAt.slice(0, 10)),
        ),
      ),
    ].sort();
    const periodDays = salesPeriod === "daily" ? 1 : salesPeriod === "weekly" ? 7 : 30;
    const periodLabel = salesPeriod === "daily" ? "日销量" : salesPeriod === "weekly" ? "周销量" : "月销量";
    const periodSales = (row: CompetitorRow, date: string) => {
      const end = new Date(`${date}T23:59:59`);
      const cutoff = new Date(end);
      cutoff.setDate(cutoff.getDate() - periodDays);
      const points = row.snapshots
        .filter((snapshot) => snapshot.salesTotal != null)
        .map((snapshot) => ({ at: new Date(snapshot.capturedAt.replace(" ", "T")), value: snapshot.salesTotal! }))
        .sort((a, b) => a.at.getTime() - b.at.getTime());
      const current = points.filter((point) => point.at <= end).at(-1);
      const previous = points.filter((point) => point.at <= cutoff).at(-1);
      return current && previous ? Math.max(0, current.value - previous.value) : null;
    };
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      grid: { left: 45, right: 25, top: 42, bottom: 35 },
      xAxis: { type: "category", data: dates },
      yAxis: [
        { type: "value", name: "价格 ₽" },
        { type: "value", name: periodLabel },
      ],
      series: chosen.flatMap((row) => [
        {
          name: `${row.name || row.productCode} 价格`,
          type: "line",
          smooth: true,
          data: dates.map(
            (d) =>
              row.snapshots.filter((s) => s.capturedAt.startsWith(d)).at(-1)
                ?.price ?? null,
          ),
        },
        {
          name: `${row.name || row.productCode} ${periodLabel}`,
          type: "line",
          smooth: true,
          yAxisIndex: 1,
          lineStyle: { type: "dashed" },
          data: dates.map((d) => periodSales(row, d)),
        },
      ]),
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [rows, selected, salesPeriod]);
  const pricedRows = rows
    .filter((row) => row.latestPrice != null)
    .sort((a, b) => (a.latestPrice ?? 0) - (b.latestPrice ?? 0));
  const priceTiers = pricedRows.map((row, index) => ({
    ...row,
    tier:
      index < pricedRows.length / 3
        ? "低价带"
        : index < (pricedRows.length * 2) / 3
          ? "中价带"
          : "高价带",
  }));
  const priceGaps = pricedRows.slice(1).map((row, index) => ({
    lower: pricedRows[index],
    upper: row,
    gap: (row.latestPrice ?? 0) - (pricedRows[index].latestPrice ?? 0),
  }));
  const largestPriceGap = priceGaps.sort((a, b) => b.gap - a.gap)[0];
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">COMPETITOR INTELLIGENCE</span>
          <h1>竞品跟踪</h1>
          <p>采集并展示竞品主图；售价和公开销量仅作为可选辅助信息</p>
        </div>
      </header>
      <div className="competitor-add">
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="粘贴 Ozon 竞品商品完整链接"
        />
        <button className="dark-button" disabled={busy || !url} onClick={add}>
          <Plus size={16} />
          添加采集任务
        </button>
      </div>
      {error && (
        <div className="error-banner">
          <AlertTriangle />
          {error}
        </div>
      )}
      <section className="card migration-note">
        <RefreshCw />
        <div>
          <h3>按任务手动采集</h3>
          <p>
            添加任务只保存竞品链接，不会立即采集。点击每张竞品卡片的“开始采集”后，软件采集商品身份和主图；直连缺少主图时才会打开专用
            Edge/Chrome。如页面要求验证，只需在弹出窗口完成验证，之后会自动校验商品、读取数据并入库，无需保存或导入
            HTML。
          </p>
        </div>
      </section>
      {latestRun && (
        <section className="card competitor-run-summary">
          <div>
            <b>最近采集批次</b>
            <small>
              {latestRun.startedAt} · {latestRun.runId}
            </small>
          </div>
          <span className="badge blue">
            完成 {latestRun.completed}/{latestRun.requested}
          </span>
          <span className="badge green">成功 {latestRun.ok}</span>
          <span className="badge orange">不完整 {latestRun.incomplete}</span>
          <span className="badge red">受阻 {latestRun.blocked}</span>
          <span>
            结构变化 {latestRun.changedLayout} · 不可访问{" "}
            {latestRun.inaccessible} · 待核验 {latestRun.ambiguousMatch}
          </span>
          <div className="competitor-rerun-actions">
            <button
              className="outline-button"
              disabled={collection?.running}
              onClick={() => void rerunBatch(false)}
            >
              <RefreshCw size={14} />
              重新运行本批次
            </button>
            <button
              className="outline-button"
              disabled={
                collection?.running ||
                latestRun.blocked +
                  latestRun.changedLayout +
                  latestRun.inaccessible +
                  latestRun.ambiguousMatch +
                  latestRun.incomplete ===
                  0
              }
              onClick={() => void rerunBatch(true)}
            >
              仅重试失败任务
            </button>
          </div>
        </section>
      )}
      {collection && (collection.running || collection.total > 0) && (
        <section className="card competitor-progress-card">
          <div className="competitor-progress-head">
            <div>
              <b>{collection.running ? "正在采集竞品" : "最近采集进度"}</b>
              <small>{collection.message}</small>
            </div>
            <div className="competitor-progress-counts">
              <span>
                任务 {collection.completed}/{collection.total}
              </span>
              <span className="badge green">成功 {collection.succeeded}</span>
              <span className="badge red">失败 {collection.failed}</span>
              {collection.currentCode && (
                <span>当前 {collection.currentCode}</span>
              )}
            </div>
          </div>
          <progress
            max={Math.max(1, collection.total)}
            value={collection.completed}
          />
          {collection.running && (
            <button
              className="stop-collection-button"
              disabled={collection.stopRequested}
              onClick={async () => {
                await stopCompetitorsCollection();
                setCollection(await competitorCollectionProgress());
              }}
            >
              {collection.stopRequested ? "正在停止" : "停止采集"}
            </button>
          )}
          <div className="competitor-task-board">
            <div className="competitor-task-board-title">
              <div>
                <b>采集任务明细</b>
                <small>
                  每个竞品独立排队、采集和停止；停止一个任务不会影响其他任务
                </small>
              </div>
              <span>{collection.tasks?.length ?? 0} 个任务</span>
            </div>
            <div className="competitor-task-list">
              {(collection.tasks ?? []).map((task, index) => {
                const canStop = ["queued", "running", "stopping"].includes(
                  task.status,
                );
                const statusText: Record<string, string> = {
                  queued: "等待中",
                  running: "采集中",
                  stopping: "停止中",
                  stopped: "已停止",
                  success: "成功",
                  incomplete: "不完整",
                  failed: "失败",
                };
                return (
                  <div
                    className={`competitor-task-row task-${task.status}`}
                    key={task.id}
                  >
                    <span className="competitor-task-index">
                      {String(index + 1).padStart(2, "0")}
                    </span>
                    <div className="competitor-task-identity">
                      <b>{task.productCode || `任务 ${task.id}`}</b>
                      <small title={task.productUrl}>{task.productUrl}</small>
                    </div>
                    <span
                      className={`competitor-task-status status-${task.status}`}
                    >
                      {statusText[task.status] ?? task.status}
                    </span>
                    <div className="competitor-task-message">
                      <b>{task.stage || "queued"}</b>
                      <small>{task.message || "等待采集"}</small>
                    </div>
                    <button
                      className="task-stop-button"
                      disabled={!canStop || task.stopRequested}
                      onClick={async () => {
                        await stopCompetitorCollectionTask(task.id);
                        setCollection(await competitorCollectionProgress());
                      }}
                    >
                      {task.stopRequested
                        ? "停止中"
                        : canStop
                          ? "停止此任务"
                          : "—"}
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      )}
      <section className="card competitor-chart-card">
        <div className="card-title">
          多竞品价格与周期销量趋势{" "}
          <span>
            <span className="sales-period-switch" role="group" aria-label="销量周期">
              {(["daily", "weekly", "monthly"] as const).map((period) => (
                <button className={salesPeriod === period ? "active" : ""} key={period} onClick={() => setSalesPeriod(period)}>
                  {period === "daily" ? "日销量" : period === "weekly" ? "周销量" : "月销量"}
                </button>
              ))}
            </span>{" "}
            <button className="outline-button demo-data-button" disabled={busy || collection?.running} onClick={async () => {
              setBusy(true);
              setError("");
              try { await seedCompetitorDemoData(); await load(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
            }}>生成月度演示数据</button>{" "}
            <button className="outline-button demo-delete-button" disabled={busy || collection?.running || !rows.some((row) => row.isDemo)} onClick={async () => {
              if (!window.confirm("确定删除当前店铺的全部竞品演示数据吗？真实采集数据不会被删除。")) return;
              setBusy(true);
              setError("");
              try { await deleteCompetitorDemoData(); await load(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
            }}>删除演示数据</button>{" "}
            <button
              className="outline-button"
              disabled={busy || collection?.running || !rows.length}
              onClick={refreshAll}
            >
              {collection?.running ? "采集中" : "运行全部采集"}
            </button>{" "}
            <span className="badge blue">{selected.length} 个已选择</span>
          </span>
        </div>
        <div id="competitor-chart" className="competitor-chart" />
      </section>
      {!!rows.length && (
        <section className="card competitor-intelligence-summary">
          <div className="competitor-alert-settings">
            <div>
              <div className="card-title">预警规则</div>
              <p>规则仅应用于当前店铺，保存后会立即重新计算预警。</p>
            </div>
            {alertSettings && (
              <div className="competitor-alert-fields">
                <label>
                  普通降价
                  <span><input type="number" min="0" max="100" step="0.5" value={alertSettings.warningDropPercent} onChange={(e) => setAlertSettings({ ...alertSettings, warningDropPercent: Number(e.target.value) })} />%</span>
                </label>
                <label>
                  严重降价
                  <span><input type="number" min="0" max="100" step="0.5" value={alertSettings.criticalDropPercent} onChange={(e) => setAlertSettings({ ...alertSettings, criticalDropPercent: Number(e.target.value) })} />%</span>
                </label>
                <label>
                  涨价机会
                  <span><input type="number" min="0" max="100" step="0.5" value={alertSettings.opportunityRisePercent} onChange={(e) => setAlertSettings({ ...alertSettings, opportunityRisePercent: Number(e.target.value) })} />%</span>
                </label>
                <button className="dark-button" disabled={savingAlerts} onClick={async () => {
                  setSavingAlerts(true);
                  setError("");
                  try {
                    await saveCompetitorAlertSettings(alertSettings);
                    await load();
                  } catch (e) {
                    setError(String(e));
                  } finally {
                    setSavingAlerts(false);
                  }
                }}><Save size={14} />{savingAlerts ? "保存中" : "保存规则"}</button>
              </div>
            )}
          </div>
          <div className="competitor-price-positioning">
            <div className="card-title">当前样本价格分层</div>
            {priceTiers.length ? (
              <div className="price-tier-list">
                {priceTiers.map((row) => <span className={`price-tier tier-${row.tier.slice(0, 1)}`} key={row.id}><b>{row.tier}</b>{row.productCode} · ₽{row.latestPrice}</span>)}
              </div>
            ) : <p>暂无可用价格。</p>}
            <small>
              {pricedRows.length < 3
                ? "价格空档分析样本不足，至少需要 3 个有价格的竞品。"
                : largestPriceGap
                  ? `最大价格空档：₽${largestPriceGap.gap.toFixed(0)}（${largestPriceGap.lower.productCode} → ${largestPriceGap.upper.productCode}）`
                  : "暂未发现价格空档。"}
              价格带仅基于当前监控样本三等分，不代表全市场分布。
            </small>
          </div>
        </section>
      )}
      {!!rows.length && (
        <section className="card competitor-alert-center">
          <div className="card-title">
            价格预警
            <span>
              {
                rows.filter((row) =>
                  ["critical", "warning"].includes(row.priceAlertLevel),
                ).length
              }{" "}
              个需要关注
            </span>
          </div>
          <div className="competitor-alert-list">
            {rows.map((row) => (
              <div
                className={`price-alert price-alert-${row.priceAlertLevel}`}
                key={row.id}
              >
                <b>{row.productCode || row.name}</b>
                <span>{row.priceAlertText}</span>
                <small>
                  当前 {row.latestPrice == null ? "—" : `₽${row.latestPrice}`} ·
                  30 日区间 {row.priceMin30d == null ? "—" : `₽${row.priceMin30d}`}–
                  {row.priceMax30d == null ? "—" : `₽${row.priceMax30d}`} · 30 日变价 {row.priceChanges30d} 次
                </small>
                {row.promotionSuspected && <em className="promotion-suspected">疑似促销（当前价低于 30 日均价 5% 以上）</em>}
              </div>
            ))}
          </div>
        </section>
      )}
      {selected.length > 0 && (
        <section className="card table-card competitor-compare-matrix">
          <div className="card-title">已选竞品对比矩阵</div>
          <table>
            <thead>
              <tr>
                <th>竞品</th>
                <th>当前价格</th>
                <th>最近变价</th>
                <th>30 日最低 / 均价 / 最高</th>
                <th>日 / 周 / 月销量</th>
                <th>预警</th>
                <th>市场信号</th>
              </tr>
            </thead>
            <tbody>
              {rows
                .filter((row) => selected.includes(row.id))
                .map((row) => (
                  <tr key={row.id}>
                    <td>
                      <b>{row.productCode || "—"}</b>
                      <small className="product">{row.name}</small>
                    </td>
                    <td>{row.latestPrice == null ? "—" : `₽${row.latestPrice}`}</td>
                    <td
                      className={
                        (row.priceChangePercent ?? 0) < 0
                          ? "price-down"
                          : (row.priceChangePercent ?? 0) > 0
                            ? "price-up"
                            : ""
                      }
                    >
                      {row.priceChangePercent == null
                        ? "—"
                        : `${row.priceChangePercent > 0 ? "+" : ""}${row.priceChangePercent.toFixed(1)}%`}
                      <small>
                        {row.priceChange == null
                          ? ""
                          : `${row.priceChange > 0 ? "+" : ""}₽${row.priceChange.toFixed(0)}`}
                      </small>
                    </td>
                    <td>
                      {row.priceMin30d == null ? "—" : `₽${row.priceMin30d}`}
                      {" / "}
                      {row.priceAvg30d == null ? "—" : `₽${row.priceAvg30d.toFixed(0)}`}
                      {" / "}
                      {row.priceMax30d == null ? "—" : `₽${row.priceMax30d}`}
                    </td>
                    <td>
                      {row.dailySales ?? "—"} / {row.weeklySales ?? "—"} /{" "}
                      {row.monthlySales ?? "—"}
                    </td>
                    <td>
                      <span className={`price-alert-tag ${row.priceAlertLevel}`}>
                        {row.priceAlertLevel === "critical"
                          ? "大幅降价"
                          : row.priceAlertLevel === "warning"
                            ? "降价关注"
                            : row.priceAlertLevel === "opportunity"
                              ? "涨价机会"
                              : row.priceAlertLevel === "stable"
                                ? "稳定"
                                : "数据不足"}
                      </span>
                    </td>
                    <td>
                      <span>30 日变价 {row.priceChanges30d} 次</span>
                      {row.promotionSuspected && <small className="promotion-suspected">疑似促销</small>}
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </section>
      )}
      <div className="competitor-grid">
        {rows.map((row) => {
          const activeTask = collection?.tasks?.find(
            (task) =>
              task.id === row.id &&
              ["queued", "running", "stopping"].includes(task.status),
          );
          return (
          <article
            className={`competitor-card ${selected.includes(row.id) ? "selected" : ""}`}
            key={row.id}
          >
            <label className="compare-check">
              <input
                type="checkbox"
                checked={selected.includes(row.id)}
                onChange={(e) =>
                  setSelected(
                    e.target.checked
                      ? [...selected, row.id]
                      : selected.filter((id) => id !== row.id),
                  )
                }
              />
              加入对比
            </label>
            <div className="competitor-image">
              {row.imageUrl ? (
                <img src={row.imageUrl} alt={row.name} />
              ) : (
                <TrendingUp />
              )}
            </div>
            <h3>{row.name || `Ozon 商品 ${row.productCode}`}</h3>
            <small>{row.productCode || "等待识别商品编号"}{row.isDemo && <span className="demo-data-tag">演示数据</span>}</small>
            <div
              className={`competitor-status status-${row.latestStatus || "pending"}`}
            >
              {row.latestStatus === "ok"
                ? "采集正常"
                : row.latestStatus === "blocked"
                  ? "验证受阻"
                  : row.latestStatus === "changed_layout"
                    ? "页面结构变化"
                    : row.latestStatus === "inaccessible"
                      ? "不可访问"
                      : row.latestStatus === "ambiguous_match"
                        ? "商品标识待核验"
                        : row.latestStatus === "incomplete"
                          ? "数据不完整"
                          : "等待采集"}
              {row.latestObservedAt && (
                <small>
                  {row.latestObservedAt} · 重试 {row.latestRetryCount} 次
                </small>
              )}
            </div>
            <div className="competitor-metrics">
              <span>
                当前售价
                <b>
                  {row.latestPrice == null
                    ? "—"
                    : `₽${row.latestPrice.toLocaleString()}`}
                </b>
                <small
                  className={
                    (row.priceChangePercent ?? 0) < 0
                      ? "price-down"
                      : (row.priceChangePercent ?? 0) > 0
                        ? "price-up"
                        : ""
                  }
                >
                  {row.priceChangePercent == null
                    ? "等待变价"
                    : `${row.priceChangePercent > 0 ? "+" : ""}${row.priceChangePercent.toFixed(1)}%`}
                </small>
              </span>
              <span>
                日销量<b>{row.dailySales ?? "—"}</b>
              </span>
              <span>
                周销量<b>{row.weeklySales ?? "—"}</b>
              </span>
              <span>
                月销量<b>{row.monthlySales ?? "—"}</b>
              </span>
            </div>
            <div className={`card-price-alert ${row.priceAlertLevel}`}>
              {row.priceAlertText}
            </div>
            <div className="competitor-market-signals">
              <span>30 日变价 {row.priceChanges30d} 次</span>
              {row.promotionSuspected && <span className="promotion-suspected">疑似促销</span>}
            </div>
            <div className="competitor-manual-sales">
              {(["daily", "weekly", "monthly"] as const).map((period) => (
                <label key={period}>
                  <span>
                    {period === "daily"
                      ? "日销量"
                      : period === "weekly"
                        ? "周销量"
                        : "月销量"}
                  </span>
                  <input
                    type="number"
                    min="0"
                    value={manualSales[row.id]?.[period] ?? ""}
                    placeholder={
                      period === "daily"
                        ? String(row.dailySales ?? "")
                        : period === "weekly"
                          ? String(row.weeklySales ?? "")
                          : String(row.monthlySales ?? "")
                    }
                    onChange={(e) =>
                      setManualSales((old) => ({
                        ...old,
                        [row.id]: {
                          daily: old[row.id]?.daily ?? "",
                          weekly: old[row.id]?.weekly ?? "",
                          monthly: old[row.id]?.monthly ?? "",
                          [period]: e.target.value,
                        },
                      }))
                    }
                  />
                </label>
              ))}
              <button
                disabled={
                  busy ||
                  !Object.values(manualSales[row.id] ?? {}).some((value) =>
                    value.trim(),
                  )
                }
                onClick={async () => {
                  const draft = manualSales[row.id];
                  const parse = (value: string | undefined, fallback: number | null) =>
                    value?.trim() ? Number(value) : fallback;
                  const daily = parse(draft?.daily, row.dailySales);
                  const weekly = parse(draft?.weekly, row.weeklySales);
                  const monthly = parse(draft?.monthly, row.monthlySales);
                  if (
                    [daily, weekly, monthly].some(
                      (value) =>
                        value != null &&
                        (!Number.isInteger(value) || value < 0),
                    )
                  ) {
                    setError("日、周、月销量必须是大于或等于 0 的整数");
                    return;
                  }
                  await setCompetitorManualMetrics(
                    row.id,
                    daily,
                    weekly,
                    monthly,
                  );
                  setManualSales((old) => {
                    const next = { ...old };
                    delete next[row.id];
                    return next;
                  });
                  await load();
                }}
              >
                保存销量
              </button>
            </div>
            {row.latestNotes && (
              <p className="competitor-notes">{row.latestNotes}</p>
            )}
            <div className="competitor-actions">
              <button
                onClick={async () => {
                  if (activeTask) {
                    await stopCompetitorCollectionTask(row.id);
                    setCollection(await competitorCollectionProgress());
                  } else {
                    await startOne(row.id);
                  }
                }}
                disabled={busy || (collection?.running && !activeTask) || activeTask?.status === "stopping"}
              >
                <RefreshCw size={14} />
                {activeTask?.status === "stopping"
                  ? "停止中"
                  : activeTask
                    ? "停止采集"
                    : "开始采集"}
              </button>
              <button
                className="remove"
                onClick={async () => {
                  await removeCompetitor(row.id);
                  await load();
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
          </article>
          );
        })}
      </div>
      {!rows.length && (
        <div className="card empty">
          尚未添加竞品。添加 Ozon 商品链接后，请在任务卡片中手动开始采集。
        </div>
      )}
      <section className="card migration-note">
        <AlertTriangle />
        <div>
          <h3>销量来源说明</h3>
          <p>
            趋势图提供日销量、周销量和月销量切换，数值由相应周期内公开累计销量快照之差计算，但不会直接展示累计销量。
            若 Ozon 页面不公开累计销量，软件显示“—”，不会使用 AI 猜测。带“演示数据”标记的内容可以一键删除。
          </p>
        </div>
      </section>
    </>
  );
}

export function ReportsPage({
  range,
  currency,
  initialTab = "summary",
}: {
  range: DateRange;
  currency: string;
  initialTab?: "summary" | "daily" | "profit" | "series" | "weekly" | "cross";
}) {
  const latestSunday = (() => {
      const d = new Date();
      d.setDate(d.getDate() - d.getDay());
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    })(),
    initialMonth = range.from.slice(0, 7),
    [month, setMonth] = useState(initialMonth),
    [weekEnd, setWeekEnd] = useState(latestSunday),
    [crossRange, setCrossRange] = useState<DateRange>(range),
    [data, setData] = useState<BusinessReport | null>(null),
    [detail, setDetail] = useState<AnalyticsDetail | null>(null),
    [crossData, setCrossData] = useState<CrossBorderReport | null>(null),
    [breakdown, setBreakdown] = useState<FinanceBreakdownRow[]>([]),
    [missingRows, setMissingRows] = useState<MissingCostRow[]>([]),
    [metricDetail, setMetricDetail] = useState<{
      title: string;
      source: string;
      rows: Array<{ label: string; value: string; note: string }>;
    } | null>(null),
    [editingMissingCost, setEditingMissingCost] =
      useState<MissingCostRow | null>(null),
    [detailFilter, setDetailFilter] = useState(""),
    [busy, setBusy] = useState(""),
    [message, setMessage] = useState(""),
    [tab, setTab] = useState<
      "summary" | "daily" | "profit" | "series" | "weekly" | "cross"
    >(initialTab);
  const monthRange = () => {
      const [y, m] = month.split("-").map(Number),
        from = `${month}-01`,
        last = new Date(y, m, 0),
        today = new Date(),
        end = last > today ? today : last;
      return {
        from,
        to: `${end.getFullYear()}-${String(end.getMonth() + 1).padStart(2, "0")}-${String(end.getDate()).padStart(2, "0")}`,
      };
    },
    weekRange = () => {
      const end = new Date(`${weekEnd}T00:00:00`),
        start = new Date(end);
      start.setDate(end.getDate() - 13);
      const f = (d: Date) =>
        `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
      return { from: f(start), to: f(end) };
    },
    effectiveRange =
      initialTab === "summary"
        ? monthRange()
        : initialTab === "weekly"
          ? weekRange()
          : initialTab === "cross"
            ? crossRange
            : range;
  useEffect(() => {
    setMessage("");
    if (tab === "summary") {
      setData(null);
      cachedReport(effectiveRange).then(setData);
    } else if (tab === "cross") {
      setCrossData(null);
      crossBorderReport(effectiveRange)
        .then(setCrossData)
        .catch((e) => setMessage(String(e)));
    } else {
      setDetail(null);
      cachedDetail(effectiveRange).then(setDetail);
    }
  }, [tab, effectiveRange.from, effectiveRange.to]);
  const availableTabs =
    initialTab === "daily"
      ? [
          ["daily", "每日产品"],
          ["profit", "产品利润"],
          ["series", "产品系列"],
        ]
      : initialTab === "summary"
        ? [["summary", "月度盈亏"]]
        : initialTab === "weekly"
          ? [["weekly", "周报明细"]]
          : [["cross", "跨境利润"]];
  useEffect(() => {
    if (!data) return;
    const el = document.getElementById("report-chart");
    if (!el) return;
    const chart = echarts.init(el);
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      grid: { left: 55, right: 25, top: 45, bottom: 30 },
      xAxis: { type: "category", data: data.daily.map((x) => x.day) },
      yAxis: [{ type: "value" }, { type: "value" }],
      series: [
        {
          name: "销售额",
          type: "bar",
          data: data.daily.map((x) => x.revenue),
          itemStyle: { color: "#4a82ef" },
        },
        {
          name: "广告花费",
          type: "line",
          yAxisIndex: 1,
          smooth: true,
          data: data.daily.map((x) => x.adSpend),
          lineStyle: { color: "#f29935" },
        },
      ],
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [data]);
  if (tab === "summary" && !data)
    return <div className="empty">正在读取月度缓存…</div>;
  const m = (v: number) => money(v, currency);
  const shiftMonth = (delta: number) => {
    const [y, mo] = month.split("-").map(Number),
      d = new Date(y, mo - 1 + delta, 1);
    setMonth(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`);
  };
  const openBreakdown = async (filter = "") => {
    setMetricDetail(null);
    setDetailFilter(filter);
    setBusy("detail");
    try {
      setBreakdown(await financeBreakdown(effectiveRange));
    } finally {
      setBusy("");
    }
  };
  const openMetric = (
    title: string,
    source: string,
    rows: Array<[string, string, string]>,
  ) => {
    setBreakdown([]);
    setDetailFilter("");
    setMetricDetail({
      title,
      source,
      rows: rows.map(([label, value, note]) => ({ label, value, note })),
    });
  };
  const openMissingCosts = async () => {
    setBusy("missing-cost");
    try {
      setMissingRows(await missingCostRows(effectiveRange));
    } finally {
      setBusy("");
    }
  };
  const syncMonth = async (kind: "seller" | "finance") => {
    setBusy(kind);
    setMessage("");
    try {
      const count =
        kind === "seller"
          ? await syncSeller(effectiveRange)
          : await syncFinance(effectiveRange);
      clearReportCache();
      setData(await businessReport(effectiveRange));
      setMessage(
        `${kind === "seller" ? "本月销量" : "本月应计费用"}已同步，写入 ${count} 行。`,
      );
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy("");
    }
  };
  const visibleBreakdown = breakdown.filter(
    (x) => !detailFilter || x.category === detailFilter,
  );
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">BUSINESS REPORTS</span>
          <h1>
            {initialTab === "summary"
              ? "月度盈亏"
              : initialTab === "weekly"
                ? "经营周报"
                : initialTab === "cross"
                  ? "跨境店铺利润"
                  : "产品数据报告"}
          </h1>
          <p>销售、广告、Finance 与成本缓存的经营预估</p>
        </div>
      </header>
      {initialTab === "weekly" && (
        <div className="month-toolbar">
          <b>本期结束日（周日）</b>
          <input
            type="date"
            value={weekEnd}
            max={latestSunday}
            onChange={(e) => {
              const d = new Date(`${e.target.value}T00:00:00`);
              d.setDate(d.getDate() + ((7 - d.getDay()) % 7));
              setWeekEnd(d.toISOString().slice(0, 10));
            }}
          />
          <button
            onClick={() => {
              const d = new Date(`${weekEnd}T00:00:00`);
              d.setDate(d.getDate() - 7);
              setWeekEnd(d.toISOString().slice(0, 10));
            }}
          >
            上一周
          </button>
          <button
            disabled={weekEnd >= latestSunday}
            onClick={() => {
              const d = new Date(`${weekEnd}T00:00:00`);
              d.setDate(d.getDate() + 7);
              setWeekEnd(d.toISOString().slice(0, 10));
            }}
          >
            下一周
          </button>
          <button onClick={() => setWeekEnd(latestSunday)}>最新完整周</button>
          <span>
            对比范围：{effectiveRange.from} 至 {effectiveRange.to}
          </span>
        </div>
      )}
      {initialTab === "summary" && (
        <div className="month-toolbar">
          <b>核算月份</b>
          <input
            type="month"
            value={month}
            onChange={(e) => setMonth(e.target.value)}
          />
          <button onClick={() => shiftMonth(-1)}>上月</button>
          <button onClick={() => shiftMonth(1)}>下月</button>
          <button disabled={!!busy} onClick={() => syncMonth("seller")}>
            同步本月销量
          </button>
          <button disabled={!!busy} onClick={() => syncMonth("finance")}>
            同步应计费用
          </button>
          <span>
            {effectiveRange.from} 至 {effectiveRange.to}
          </span>
        </div>
      )}
      {initialTab === "cross" && (
        <div className="month-toolbar sync-range-toolbar">
          <b>利润日期</b>
          <label>
            开始
            <input
              type="date"
              value={crossRange.from}
              max={crossRange.to}
              onChange={(e) =>
                setCrossRange((x) => ({ ...x, from: e.target.value }))
              }
            />
          </label>
          <em>至</em>
          <label>
            结束
            <input
              type="date"
              value={crossRange.to}
              min={crossRange.from}
              max={new Date().toISOString().slice(0, 10)}
              onChange={(e) =>
                setCrossRange((x) => ({ ...x, to: e.target.value }))
              }
            />
          </label>
          <button
            onClick={() => {
              const today = new Date().toISOString().slice(0, 10);
              setCrossRange({ from: today, to: today });
            }}
          >
            日利润
          </button>
          <button
            onClick={() => {
              const to = new Date(),
                from = new Date();
              from.setDate(to.getDate() - 6);
              setCrossRange({
                from: from.toISOString().slice(0, 10),
                to: to.toISOString().slice(0, 10),
              });
            }}
          >
            周利润
          </button>
          <button onClick={() => setCrossRange(monthRange())}>月利润</button>
        </div>
      )}
      {message && <div className="sync-message">{message}</div>}
      {availableTabs.length > 1 && (
        <div className="tabs report-tabs">
          {availableTabs.map(([key, label]) => (
            <button
              key={key}
              className={tab === key ? "selected" : ""}
              onClick={() => setTab(key as typeof tab)}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      {tab === "summary" && data && (
        <>
          <div className="report-hero">
            <div
              role="button"
              tabIndex={0}
              onClick={() =>
                openMetric("税前利润", "Finance 结算与已交付成本", [
                  [
                    "Finance 已结算净额",
                    m(data.financeNet),
                    "Finance amount 汇总",
                  ],
                  [
                    "减：采购成本",
                    m(-data.purchaseCost),
                    `${data.costedUnits} 件已核算，${data.missingCostUnits} 件缺成本`,
                  ],
                  ["减：头程成本", m(-data.firstMileCost), "按已交付件数核算"],
                  [
                    "税前利润",
                    data.settledProfit == null
                      ? "成本未完整"
                      : m(data.settledProfit),
                    data.missingCostUnits
                      ? `仍有 ${data.missingCostUnits} 件、${data.missingCostSkus} 个 SKU 缺成本`
                      : "成本完整",
                  ],
                ])
              }
            >
              <span>税前利润</span>
              <strong
                className={
                  (data.settledProfit ?? data.estimatedProfit) < 0
                    ? "negative"
                    : ""
                }
              >
                {data.settledProfit == null
                  ? "成本未完整"
                  : m(data.settledProfit)}
              </strong>
              <small>Finance 净额 − 已交付商品采购成本 − 已交付商品头程</small>
            </div>
            <div
              role="button"
              tabIndex={0}
              onClick={() =>
                openMetric("Finance 已结算净额", "Ozon Finance API", [
                  ["销售/退货应计", m(data.salesReturns), "accruals_for_sale"],
                  [
                    "应计费用",
                    m(data.accrualFees),
                    "佣金、物流、广告及其他服务",
                  ],
                  [
                    "其他收入/调整",
                    m(data.otherAdjustments),
                    "Cash Flow others",
                  ],
                  [
                    "合计",
                    m(data.financeNet),
                    `${data.financeOperations} 笔 Finance 记录`,
                  ],
                ])
              }
            >
              <span>Finance 已结算净额</span>
              <strong>{m(data.financeNet)}</strong>
              <small>用于与经营预估对账</small>
            </div>
            <div
              role="button"
              tabIndex={0}
              onClick={() =>
                openMetric("税后利润", "本地税费设置", [
                  [
                    "税前利润",
                    data.settledProfit == null
                      ? "成本未完整"
                      : m(data.settledProfit),
                    "成本完整后才计算",
                  ],
                  [
                    `税额（${data.taxRate}%）`,
                    m(data.taxAmount),
                    "按销售额计算",
                  ],
                  [
                    `提现手续费（${data.payoutFeeRate}%）`,
                    m(data.payoutFee),
                    "按 Finance 净额计算",
                  ],
                  [
                    "税后利润",
                    data.afterTaxProfit == null ? "—" : m(data.afterTaxProfit),
                    "税前利润 + 税额 + 提现费",
                  ],
                ])
              }
            >
              <span>税后利润</span>
              <strong>
                {data.afterTaxProfit == null ? "—" : m(data.afterTaxProfit)}
              </strong>
              <small>
                税率 {data.taxRate}% · 提现费 {data.payoutFeeRate}%
              </small>
            </div>
          </div>
          <section className="card report-chart-card">
            <div className="card-title">销售额与广告花费趋势</div>
            <div id="report-chart" className="report-chart" />
          </section>
          <section className="card report-breakdown">
            <h3>
              期间构成 <small>点击任意指标查看明细与数据口径</small>
            </h3>
            <div>
              <button
                onClick={() =>
                  openMetric(
                    "销售额（下单口径）",
                    "Seller Analytics",
                    data.daily.map(
                      (x) =>
                        [x.day, m(x.revenue), `${x.orders} 件`] as [
                          string,
                          string,
                          string,
                        ],
                    ),
                  )
                }
              >
                销售额（下单口径）<b>{m(data.revenue)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric(
                    "订单量",
                    "Seller Analytics",
                    data.daily.map(
                      (x) =>
                        [x.day, `${x.orders} 件`, m(x.revenue)] as [
                          string,
                          string,
                          string,
                        ],
                    ),
                  )
                }
              >
                订单量<b>{data.orders}</b>
              </button>
              <button
                onClick={() =>
                  openMetric("销售/退货应计", "Ozon Finance API", [
                    [
                      "应计金额",
                      m(data.salesReturns),
                      "accruals_for_sale 汇总",
                    ],
                    [
                      "Finance 记录",
                      `${data.financeOperations} 笔`,
                      "当前月份",
                    ],
                  ])
                }
              >
                销售/退货应计<b>{m(data.salesReturns)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric("应计费用", "Ozon Finance API", [
                    ["Finance 净额", m(data.financeNet), "amount 汇总"],
                    [
                      "减：销售/退货应计",
                      m(-data.salesReturns),
                      "accruals_for_sale",
                    ],
                    [
                      "减：其他收入/调整",
                      m(-data.otherAdjustments),
                      "Cash Flow others",
                    ],
                    ["应计费用", m(data.accrualFees), "差额核算"],
                  ])
                }
              >
                应计费用<b>{m(data.accrualFees)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric("其他收入/调整", "Ozon Cash Flow API", [
                    [
                      "其他收入/调整",
                      m(data.otherAdjustments),
                      "others.total 或 others.items 汇总",
                    ],
                  ])
                }
              >
                其他收入/调整<b>{m(data.otherAdjustments)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric(
                    "Performance 广告",
                    "Ozon Performance API",
                    data.daily.map(
                      (x) =>
                        [x.day, m(x.adSpend), "日广告花费"] as [
                          string,
                          string,
                          string,
                        ],
                    ),
                  )
                }
              >
                Performance 广告<b>{m(data.adSpend)}</b>
              </button>
              <button onClick={() => openBreakdown("advertising")}>
                Finance 广告应计<b>{m(data.financeAdvertising)}</b>
              </button>
              <button
                className={data.missingCostSkus ? "missing-cost" : ""}
                onClick={() =>
                  openMetric("采购成本", "已交付件数 × SKU 单位采购成本", [
                    [
                      "已核算件数",
                      `${data.costedUnits} 件`,
                      "Posting 交付事件优先，Finance 已交付记录兜底",
                    ],
                    [
                      "缺成本件数",
                      `${data.missingCostUnits} 件`,
                      `${data.missingCostSkus} 个 SKU`,
                    ],
                    [
                      "采购成本",
                      m(data.purchaseCost),
                      "仅汇总成本资料完整的已交付商品",
                    ],
                  ])
                }
              >
                采购成本
                <b>
                  {data.missingCostSkus
                    ? `缺 ${data.missingCostSkus} 个 SKU`
                    : m(data.purchaseCost)}
                </b>
              </button>
              <button
                className={data.missingCostSkus ? "missing-cost" : ""}
                onClick={() =>
                  openMetric("头程成本", "已交付件数 × SKU 单件头程", [
                    [
                      "已核算件数",
                      `${data.costedUnits} 件`,
                      "与采购成本使用相同交付口径",
                    ],
                    [
                      "缺成本件数",
                      `${data.missingCostUnits} 件`,
                      `${data.missingCostSkus} 个 SKU`,
                    ],
                    [
                      "头程成本",
                      m(data.firstMileCost),
                      "仅汇总成本资料完整的已交付商品",
                    ],
                  ])
                }
              >
                头程成本
                <b>
                  {data.missingCostSkus
                    ? "成本资料未完整"
                    : m(data.firstMileCost)}
                </b>
              </button>
              <button onClick={() => openBreakdown("commission")}>
                平台佣金<b>{m(data.commission)}</b>
              </button>
              <button onClick={() => openBreakdown("delivery")}>
                配送费用<b>{m(data.deliveryFees)}</b>
              </button>
              <button onClick={() => openBreakdown("return_logistics")}>
                退货物流<b>{m(data.returnFees)}</b>
              </button>
              <button onClick={() => openBreakdown("acquiring")}>
                收单支付<b>{m(data.acquiring)}</b>
              </button>
              <button onClick={() => openBreakdown("storage")}>
                仓储包装<b>{m(data.storagePackaging)}</b>
              </button>
              <button onClick={() => openBreakdown("penalties")}>
                罚款与调整<b>{m(data.penaltiesAdjustments)}</b>
              </button>
              <button onClick={() => openBreakdown("other")}>
                其他 Finance 项目<b>{m(data.otherFinanceFees)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric("税额", "本地税费设置", [
                    ["计税销售额", m(data.revenue), "Seller 下单销售额"],
                    ["税率", `${data.taxRate}%`, "连接设置中的本地税率"],
                    ["税额", m(data.taxAmount), "计税销售额 × 税率"],
                  ])
                }
              >
                税额<b>{m(data.taxAmount)}</b>
              </button>
              <button
                onClick={() =>
                  openMetric("提现手续费", "本地税费设置", [
                    ["Finance 净额", m(data.financeNet), "仅正数作为计费基数"],
                    ["费率", `${data.payoutFeeRate}%`, "连接设置中的提现费率"],
                    ["提现手续费", m(data.payoutFee), "Finance 净额 × 费率"],
                  ])
                }
              >
                提现手续费<b>{m(data.payoutFee)}</b>
              </button>
              <button onClick={() => openBreakdown()}>
                全部 Finance 明细<b>{data.financeOperations} 笔</b>
              </button>
              <button
                onClick={() =>
                  openMetric("精确归属 SKU", "Ozon Finance API items[]", [
                    [
                      "精确归属",
                      `${data.exactSkuOperations} 笔`,
                      "一笔 Finance 操作只包含一个 SKU",
                    ],
                    [
                      "全部 Finance",
                      `${data.financeOperations} 笔`,
                      "当前月份",
                    ],
                  ])
                }
              >
                精确归属 SKU<b>{data.exactSkuOperations} 笔</b>
              </button>
              <button
                onClick={() =>
                  openMetric("未分摊 Finance", "Ozon Finance API items[]", [
                    [
                      "未分摊记录",
                      `${data.unallocatedOperations} 笔`,
                      "无 SKU 或一笔包含多个 SKU",
                    ],
                    [
                      "未分摊金额",
                      m(data.unallocatedFinanceAmount),
                      "保留在店铺级损益，不猜测分摊",
                    ],
                  ])
                }
              >
                未分摊 Finance
                <b>
                  {data.unallocatedOperations} 笔 ·{" "}
                  {m(data.unallocatedFinanceAmount)}
                </b>
              </button>
              <button
                onClick={() =>
                  openMetric(
                    "Cash Flow 对账差额",
                    "Ozon Finance / Cash Flow API",
                    [
                      [
                        "Cash Flow 汇总",
                        m(data.cashFlowReportedTotal),
                        "orders + returns + commission + delivery/return + services",
                      ],
                      [
                        "Finance 净额",
                        m(data.financeNet),
                        "Finance amount 汇总",
                      ],
                      [
                        "对账差额",
                        data.reconciliationDifference == null
                          ? "无汇总缓存"
                          : m(data.reconciliationDifference),
                        "Cash Flow 汇总 − Finance 净额",
                      ],
                    ],
                  )
                }
              >
                Cash Flow 对账差额
                <b>
                  {data.reconciliationDifference == null
                    ? "无汇总缓存"
                    : m(data.reconciliationDifference)}
                </b>
              </button>
              <button
                onClick={() =>
                  openMetric("已核算销量", "Finance 已交付与 SKU 成本表", [
                    [
                      "成本完整",
                      `${data.costedUnits} 件`,
                      "采购成本与头程成本均存在",
                    ],
                    [
                      "成本缺失",
                      `${data.missingCostUnits} 件`,
                      `${data.missingCostSkus} 个 SKU`,
                    ],
                    [
                      "交付总量",
                      `${data.costedUnits + data.missingCostUnits} 件`,
                      "Finance 已交付优先；无记录时回退 Posting 妥投",
                    ],
                  ])
                }
              >
                已核算销量<b>{data.costedUnits} 件</b>
              </button>
              <button
                onClick={openMissingCosts}
                disabled={!data.missingCostSkus}
              >
                缺成本 SKU<b>{data.missingCostSkus}</b>
              </button>
            </div>
            <p>
              Finance 恒等式：已结算净额 = 销售/退货应计 + 应计费用 +
              其他收入/调整。Performance 广告仅用于经营趋势；Finance
              广告已包含在应计费用中，不会重复扣除。
            </p>
          </section>
          {metricDetail && (
            <section className="card table-card finance-detail">
              <div className="card-title">
                {metricDetail.title} 明细
                <button onClick={() => setMetricDetail(null)}>关闭</button>
              </div>
              <p className="metric-detail-source">
                数据来源：{metricDetail.source}
              </p>
              <table>
                <thead>
                  <tr>
                    <th>项目 / 日期</th>
                    <th>数值</th>
                    <th>口径说明</th>
                  </tr>
                </thead>
                <tbody>
                  {metricDetail.rows.map((row, index) => (
                    <tr key={`${row.label}-${index}`}>
                      <td>{row.label}</td>
                      <td>{row.value}</td>
                      <td>{row.note}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}
          {(busy === "detail" || breakdown.length > 0) && (
            <section className="card table-card finance-detail">
              <div className="card-title">
                {detailFilter
                  ? visibleBreakdown[0]?.categoryLabel || "费用"
                  : "全部 Finance"}{" "}
                明细{" "}
                <button
                  onClick={() => {
                    setBreakdown([]);
                    setDetailFilter("");
                  }}
                >
                  关闭
                </button>
              </div>
              {busy === "detail" ? (
                <div className="empty">正在读取逐笔分类缓存…</div>
              ) : (
                <table>
                  <thead>
                    <tr>
                      <th>费用大类</th>
                      <th>费用项目</th>
                      <th>API 字段/服务名</th>
                      <th>记录数</th>
                      <th>金额</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibleBreakdown.map((x) => (
                      <tr key={`${x.category}-${x.apiName}-${x.name}`}>
                        <td>{x.categoryLabel}</td>
                        <td>{x.name}</td>
                        <td>{x.apiName}</td>
                        <td>{x.rowsCount}</td>
                        <td>{m(x.amount)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>
          )}
          {(busy === "missing-cost" || missingRows.length > 0) && (
            <section className="card table-card finance-detail">
              <div className="card-title">
                缺成本 SKU 明细（{missingRows.length} 个）
                <button onClick={() => setMissingRows([])}>关闭</button>
              </div>
              {busy === "missing-cost" ? (
                <div className="empty">正在读取当前月份缺成本 SKU…</div>
              ) : (
                <table>
                  <thead>
                    <tr>
                      <th>货号 / Ozon SKU</th>
                      <th>期间销量</th>
                      <th>成本核算缺失项</th>
                      <th>物流资料提示</th>
                      <th>操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {missingRows.map((row) => (
                      <tr
                        key={row.sku}
                        onDoubleClick={() => setEditingMissingCost(row)}
                        style={{ cursor: "pointer" }}
                        title="双击编辑该 SKU 成本"
                      >
                        <td>
                          <b>{row.offerId || "—"}</b>
                          <small>{row.sku}</small>
                        </td>
                        <td>{row.units} 件</td>
                        <td className="missing-cost">
                          {[
                            row.missingPurchase && "采购成本",
                            row.missingFirstMile && "头程成本",
                          ]
                            .filter(Boolean)
                            .join("、") || "—"}
                        </td>
                        <td>
                          {[
                            row.missingWeight && "重量",
                            row.missingDimensions && "尺寸",
                          ]
                            .filter(Boolean)
                            .join("、") || "完整"}
                        </td>
                        <td>
                          <button
                            className="outline-button"
                            onClick={() => setEditingMissingCost(row)}
                          >
                            编辑成本
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>
          )}
          {editingMissingCost && (
            <MissingCostEditor
              row={editingMissingCost}
              close={() => setEditingMissingCost(null)}
              saved={async () => {
                clearReportCache();
                setEditingMissingCost(null);
                setBusy("missing-cost");
                try {
                  const [nextRows, nextReport] = await Promise.all([
                    missingCostRows(effectiveRange),
                    businessReport(effectiveRange),
                  ]);
                  setMissingRows(nextRows);
                  setData(nextReport);
                  setMessage(
                    `SKU ${editingMissingCost.sku} 成本已保存，月度盈亏已重新核算。`,
                  );
                } finally {
                  setBusy("");
                }
              }}
            />
          )}{" "}
        </>
      )}
      {tab === "cross" && <CrossBorderView data={crossData} />}
      {tab !== "summary" && tab !== "cross" && (
        <AnalysisDetailView tab={tab} detail={detail} money={m} />
      )}
    </>
  );
}

function CrossBorderView({ data }: { data: CrossBorderReport | null }) {
  useEffect(() => {
    if (!data) return;
    const el = document.getElementById("cross-profit-trend");
    if (!el) return;
    const chart = echarts.init(el);
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      grid: { left: 60, right: 45, top: 42, bottom: 30 },
      xAxis: { type: "category", data: data.daily.map((x) => x.day) },
      yAxis: { type: "value" },
      series: [
        {
          name: "销售额",
          type: "bar",
          data: data.daily.map((x) => x.revenueCny),
        },
        {
          name: "日利润",
          type: "line",
          smooth: true,
          data: data.daily.map((x) => x.profitCny),
        },
        {
          name: "广告费",
          type: "line",
          smooth: true,
          data: data.daily.map((x) => x.adSpendCny),
        },
      ],
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [data]);
  if (!data) return <div className="card empty">正在后台读取跨境利润缓存…</div>;
  const cny = (v: number) =>
    `¥${v.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
  const rate = (v: number | null) =>
    v == null ? "历史样本不足" : `${(v * 100).toFixed(2)}%`;
  return (
    <>
      <div className="report-hero">
        <div>
          <span>销售额 CNY</span>
          <strong>{cny(data.revenueCny)}</strong>
          <small>{data.units} 件</small>
        </div>
        <div>
          <span>全店广告</span>
          <strong>{cny(data.adSpendCny)}</strong>
          <small>仅汇总扣除，不分摊单品</small>
        </div>
        <div>
          <span>预估佣金 + 收单</span>
          <strong>{cny(data.estimatedPlatformFeesCny)}</strong>
          <small>
            佣金 {rate(data.commissionRate)} · 收单 {rate(data.acquiringRate)}
          </small>
        </div>
        <div>
          <span>采购 + 预估运费</span>
          <strong>{cny(data.purchaseAndFreightCny)}</strong>
          <small>缺 {data.missingCostSkus} 个 SKU</small>
        </div>
        <div>
          <span>实时预估利润</span>
          <strong>
            {data.profitCny == null
              ? "成本/历史费率未完整"
              : cny(data.profitCny)}
          </strong>
          <small>下单销售额口径</small>
        </div>
        <div>
          <span>Finance 已结算净额</span>
          <strong>
            {data.financeAvailable ? cny(data.settledFinanceNetCny) : "—"}
          </strong>
          <small>仅用于对账</small>
        </div>
      </div>
      <div className="sync-message">
        1 CNY = {Number(data.rubPerCny.toFixed(4))} RUB · 履约缓存 FBP{" "}
        {data.fbpOrders} / FBS {data.rfbsOrders} / WHD {data.whdOrders} ·{" "}
        {data.dateFrom} 至 {data.dateTo}
      </div>
      <section className="card report-chart-card">
        <div className="card-title">跨境销售额、广告与日利润趋势</div>
        <div id="cross-profit-trend" className="report-chart" />
      </section>
      <section className="card table-card report-detail">
        <table>
          <thead>
            <tr>
              <th>货号 / SKU</th>
              <th>销量 / 履约</th>
              <th>FBP / FBS / WHD</th>
              <th>销售额 CNY</th>
              <th>单件售价</th>
              <th>采购 / 重量</th>
              <th>定价跨境运费</th>
              <th>采购合计</th>
              <th>运费合计</th>
              <th>预估佣金+收单</th>
              <th>实时商品贡献</th>
              <th>Finance 对账</th>
            </tr>
          </thead>
          <tbody>
            {data.rows.map((x) => (
              <tr key={x.sku}>
                <td>
                  <b>{x.offerId || "—"}</b>
                  <small>{x.sku}</small>
                </td>
                <td>
                  <b>{x.units}</b>
                  <small>履约单 {x.fulfillmentOrders}</small>
                </td>
                <td>
                  {x.fbpOrders} / {x.rfbsOrders} / {x.whdOrders}
                </td>
                <td>{cny(x.revenueCny)}</td>
                <td>{cny(x.sellingPriceCny)}</td>
                <td>
                  {x.purchaseCostCny == null ? "—" : cny(x.purchaseCostCny)}
                  <small>
                    {x.weightKg == null ? "重量未填" : `${x.weightKg} kg`}
                  </small>
                </td>
                <td>
                  {x.freightUnitCny == null
                    ? "超出区间/缺重量"
                    : cny(x.freightUnitCny)}
                </td>
                <td>
                  {x.purchaseTotalCny == null ? "—" : cny(x.purchaseTotalCny)}
                </td>
                <td>
                  {x.freightTotalCny == null ? "—" : cny(x.freightTotalCny)}
                </td>
                <td>
                  {x.estimatedPlatformFeesCny == null
                    ? "历史费率不足"
                    : cny(x.estimatedPlatformFeesCny)}
                </td>
                <td>
                  {x.contributionCny == null
                    ? "数据不足"
                    : cny(x.contributionCny)}
                </td>
                <td>
                  {x.financeSettledCny == null ? "—" : cny(x.financeSettledCny)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!data.rows.length && (
          <div className="empty">
            当前日期范围没有出单数据。请先同步 Seller 销量；Finance
            和广告分别用于费率、对账与全店广告。
          </div>
        )}
      </section>
    </>
  );
}

function AnalysisDetailView({
  tab,
  detail,
  money,
}: {
  tab: "daily" | "profit" | "series" | "weekly" | "cross";
  detail: AnalyticsDetail | null;
  money: (v: number) => string;
}) {
  const [page, setPage] = useState(0);
  useEffect(() => setPage(0), [tab, detail]);
  const rows =
    tab === "daily"
      ? (detail?.dailyProducts ?? []).map((x) => [
          x.day,
          x.offerId || "—",
          x.sku,
          x.units,
          money(x.revenue),
          money(x.adSpend),
          x.adOrders,
          x.adCostPerOrder == null ? "—" : money(x.adCostPerOrder),
          x.tacos == null ? "—" : `${x.tacos.toFixed(2)}%`,
          x.costComplete && x.estimatedProfit != null
            ? money(x.estimatedProfit)
            : "缺少成本",
        ])
      : tab === "profit"
        ? (detail?.products ?? []).map((x) => [
            x.offerId || "—",
            x.sku,
            x.units,
            money(x.revenue),
            money(x.adSpend),
            x.costComplete ? money(x.estimatedProfit ?? 0) : "缺少成本",
          ])
        : tab === "series"
          ? (detail?.series ?? []).map((x) => [
              x.periodType,
              x.period,
              x.series,
              x.skuCount,
              x.units,
              money(x.revenue),
            ])
          : tab === "weekly"
            ? (detail?.weekly ?? []).map((x, i, all) => [
                x.period,
                x.units,
                money(x.revenue),
                money(x.adSpend),
                x.adOrders,
                `${x.adOrderShare.toFixed(2)}%`,
                `${x.acots.toFixed(2)}%`,
                x.returns,
                x.cancellations,
                all[i + 1]?.revenue
                  ? `${((x.revenue / all[i + 1].revenue - 1) * 100).toFixed(2)}%`
                  : "—",
                all[i + 1]?.units
                  ? `${((x.units / all[i + 1].units - 1) * 100).toFixed(2)}%`
                  : "—",
              ])
            : (detail?.products ?? []).map((x) => [
                x.offerId || "—",
                x.sku,
                x.units,
                money(x.revenue),
                x.crossBorderFreight == null
                  ? "缺少重量/不适用"
                  : money(x.crossBorderFreight),
                x.estimatedProfit == null || x.crossBorderFreight == null
                  ? "—"
                  : money(x.estimatedProfit - x.crossBorderFreight),
              ]);
  const headers =
    tab === "daily"
      ? [
          "日期",
          "货号",
          "Ozon SKU",
          "销量",
          "销售额",
          "广告费",
          "广告订单",
          "每单广告费",
          "TACOS",
          "当天预估利润",
        ]
      : tab === "profit"
        ? ["商品", "SKU / 货号", "销量", "销售额", "广告", "预估利润"]
        : tab === "series"
          ? ["粒度", "周期", "产品系列", "SKU 数", "销量", "销售额"]
          : tab === "weekly"
            ? [
                "周",
                "销量",
                "销售额",
                "广告花费",
                "广告订单",
                "广告单占比",
                "ACoTS",
                "退货",
                "取消",
                "销售额环比",
                "销量环比",
              ]
            : [
                "商品",
                "SKU / 货号",
                "销量",
                "销售额",
                "跨境运费",
                "扣运费后贡献",
              ];
  const pages = Math.max(1, Math.ceil(rows.length / 100)),
    visible = rows.slice(page * 100, page * 100 + 100);
  return (
    <>
      <section className="card table-card report-detail">
        <table>
          <thead>
            <tr>
              {headers.map((x) => (
                <th key={x}>{x}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {visible.map((row, i) => (
              <tr key={i}>
                {row.map((value, j) => (
                  <td key={j}>
                    <b>{value}</b>
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {!rows.length && (
          <div className="empty">
            {detail
              ? "当前日期范围没有可汇总的真实数据。"
              : "正在从本地持久缓存读取明细…"}
          </div>
        )}
      </section>
      {tab === "weekly" && (detail?.weeklyDaily?.length ?? 0) > 0 && (
        <section className="card table-card report-detail">
          <div className="card-title">本周每日数据</div>
          <table>
            <thead>
              <tr>
                <th>日期</th>
                <th>销量</th>
                <th>销售额</th>
                <th>广告花费</th>
                <th>广告订单</th>
                <th>广告单占比</th>
                <th>ACoTS</th>
                <th>退货</th>
                <th>取消</th>
              </tr>
            </thead>
            <tbody>
              {detail!.weeklyDaily.map((x) => (
                <tr key={x.day}>
                  <td>
                    <b>{x.day}</b>
                  </td>
                  <td>{x.units}</td>
                  <td>{money(x.revenue)}</td>
                  <td>{money(x.adSpend)}</td>
                  <td>{x.adOrders}</td>
                  <td>{x.adOrderShare.toFixed(2)}%</td>
                  <td>{x.acots.toFixed(2)}%</td>
                  <td>{x.returns}</td>
                  <td>{x.cancellations}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      {rows.length > 100 && (
        <div className="table-pagination">
          <button disabled={page === 0} onClick={() => setPage(page - 1)}>
            上一页
          </button>
          <span>
            第 {page + 1} / {pages} 页 · 共 {rows.length} 条
          </span>
          <button
            disabled={page + 1 >= pages}
            onClick={() => setPage(page + 1)}
          >
            下一页
          </button>
        </div>
      )}
    </>
  );
}

export function AiPage({ range }: { range: DateRange }) {
  const [question, setQuestion] = useState(
      "请分析当前经营健康度、利润风险、广告效率，并给出按优先级排序的行动建议。",
    ),
    [answer, setAnswer] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const run = async () => {
    setBusy(true);
    setError("");
    try {
      setAnswer(await aiAnalysis(range, question));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">LOCAL-FIRST AI ANALYST</span>
          <h1>AI 经营分析</h1>
          <p>基于当前店铺、日期范围和本地真实汇总生成经营建议</p>
        </div>
        <button
          className="dark-button"
          disabled={busy || !question.trim()}
          onClick={run}
        >
          <TrendingUp size={16} />
          {busy ? "分析中" : "开始分析"}
        </button>
      </header>
      <section className="card ai-panel">
        <div className="card-title">
          分析目标{" "}
          <span className="badge blue">
            {range.from} 至 {range.to}
          </span>
        </div>
        <textarea
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          rows={5}
        />
        <p className="ai-privacy">
          仅发送经营汇总和销量前 20 的商品摘要，不发送 API
          密钥、订单号或买家信息。
        </p>
      </section>
      {error && (
        <div className="error-banner">
          <AlertTriangle />
          {error}
        </div>
      )}
      <section className="card ai-answer">
        <div className="card-title">分析结果</div>
        {answer ? (
          <div className="ai-copy">{answer}</div>
        ) : (
          <div className="empty">
            配置 OpenAI 兼容的 AI
            服务后，可在这里获得基于真实数据的诊断；系统不会用演示数据替代。
          </div>
        )}
      </section>
    </>
  );
}

export function SupplyPage() {
  const today = new Date().toISOString().slice(0, 10),
    later = new Date(Date.now() + 14 * 86400000).toISOString().slice(0, 10);
  const [rows, setRows] = useState<SupplyOrder[]>([]),
    [plans, setPlans] = useState<SupplyClusterPlan[]>([]),
    [planQuery, setPlanQuery] = useState(""),
    [targetDays, setTargetDays] = useState(30),
    [planDrafts, setPlanDrafts] = useState<Record<string, string>>({}),
    [chosenPlan, setChosenPlan] = useState<SupplyClusterPlan | null>(null),
    [supplyMode, setSupplyMode] = useState<"CROSSDOCK" | "DIRECT">("CROSSDOCK"),
    [selected, setSelected] = useState<SupplyOrder | null>(null),
    [slots, setSlots] = useState<SupplyTimeslot[]>([]),
    [from, setFrom] = useState(today),
    [to, setTo] = useState(later),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const mounted = useRef(true);
  const load = async () => {
    setBusy(true);
    setError("");
    try {
      const next = await supplyOrders();
      if (mounted.current) setRows(next);
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const loadPlans = async () => {
    setError("");
    try {
      const next = await supplyClusterPlans(targetDays, planQuery);
      if (mounted.current) setPlans(next);
    } catch (e) {
      if (mounted.current) setError(String(e));
    }
  };
  useEffect(() => {
    mounted.current = true;
    void load();
    void loadPlans();
    return () => {
      mounted.current = false;
    };
  }, []);
  const findSlots = async (row: SupplyOrder) => {
    setSelected(row);
    setBusy(true);
    setError("");
    try {
      const next = await supplyTimeslots(row.orderId, from, to);
      if (mounted.current) setSlots(next);
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  const book = async (slot: SupplyTimeslot) => {
    if (!selected) return;
    const prompt = `确认预约供应单 ${selected.orderNumber || selected.orderId}\n${slot.from} 至 ${slot.to}\n\n该操作将真实写入 Ozon，且不会自动重试。请输入“确认预约”继续：`;
    if (window.prompt(prompt) !== "确认预约") return;
    setBusy(true);
    try {
      const result = await bookSupplyTimeslot(
        selected.orderId,
        slot.from,
        slot.to,
        "确认预约",
      );
      if (mounted.current) {
        alert(result);
        await load();
      }
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">FBO SUPPLY APPOINTMENTS</span>
          <h1>约仓计划</h1>
          <p>真实读取活动供应单、越库集群并查询或修改预约时段</p>
        </div>
        <button className="dark-button" onClick={load} disabled={busy}>
          <RefreshCw size={16} />
          {busy ? "请求中" : "读取供应单"}
        </button>
      </header>
      {error && (
        <div className="error-banner">
          <AlertTriangle />
          {error}
        </div>
      )}
      <div className="supply-layout">
        <section className="card table-card" style={{ gridColumn: "1 / -1" }}>
          <div className="card-title">
            从库存集群生成约仓计划
            <span>库存管理中保存的配送量会在这里直接作为计划数量</span>
          </div>
          <div className="inventory-toolbar">
            <input value={planQuery} onChange={(e) => setPlanQuery(e.target.value)} placeholder="搜索 SKU、货号、商品或集群" />
            <label>目标天数 <input type="number" min="1" max="365" value={targetDays} onChange={(e) => setTargetDays(Math.max(1, Number(e.target.value) || 1))} /></label>
            <button className="outline-button" onClick={() => void loadPlans()}>读取集群计划</button>
          </div>
          <table>
            <thead><tr><th>SKU / 商品</th><th>配送集群</th><th>可售 / 在途 / 已申请</th><th>日均销量</th><th>建议量</th><th>约仓数量</th><th>操作</th></tr></thead>
            <tbody>{plans.map((plan) => {
              const key = `${plan.sku}|${plan.macrolocalClusterId}`;
              const draft = planDrafts[key] ?? String(plan.plannedQty);
              return <tr key={key} className={chosenPlan && `${chosenPlan.sku}|${chosenPlan.macrolocalClusterId}` === key ? "row-selected" : ""}>
                <td><b>{plan.offerId || plan.sku}</b><small>{plan.productName} · SKU {plan.sku}</small></td>
                <td><b>{plan.clusterName}</b><small>{plan.macrolocalClusterId}</small></td>
                <td>{plan.availableStock} / {plan.transitStock} / {plan.requestedStock}</td>
                <td>{plan.dailySales.toFixed(1)}</td><td>{plan.recommendedQty}</td>
                <td><input type="number" min="0" value={draft} onChange={(e) => setPlanDrafts((old) => ({ ...old, [key]: e.target.value }))} /></td>
                <td><button className="outline-button" onClick={async () => {
                  const quantity = Number(draft);
                  if (!Number.isInteger(quantity) || quantity < 0) { setError("约仓数量必须是大于或等于 0 的整数"); return; }
                  await saveSupplyClusterPlan(plan.sku, plan.macrolocalClusterId, quantity, targetDays);
                  const saved = { ...plan, plannedQty: quantity, targetDays, planSaved: true };
                  setChosenPlan(saved); await loadPlans();
                }}>{plan.planSaved ? "保存并选择" : "采用并选择"}</button></td>
              </tr>;
            })}</tbody>
          </table>
          {!plans.length && <div className="empty">库存同步后，可按 SKU 与配送集群选择约仓数量。</div>}
          {chosenPlan && <div className="migration-note">
            <Truck />
            <div><h3>已选：{chosenPlan.offerId || chosenPlan.sku} → {chosenPlan.clusterName}，{chosenPlan.plannedQty} 件</h3>
              <p>{supplyMode === "CROSSDOCK" ? "越库 CROSSDOCK：必须选择 Ozon 发货点，货物先送往越库点，再由平台转运到目的仓。" : "直送 DIRECT：直接送达系统计算出的目的仓，不选择越库发货点。"}</p>
            </div>
            <div className="sales-period-switch" role="group" aria-label="约仓模式">
              <button className={supplyMode === "CROSSDOCK" ? "active" : ""} onClick={() => setSupplyMode("CROSSDOCK")}>越库</button>
              <button className={supplyMode === "DIRECT" ? "active" : ""} onClick={() => setSupplyMode("DIRECT")}>直送</button>
            </div>
          </div>}
        </section>
        <section className="card table-card">
          <table>
            <thead>
              <tr>
                <th>供应单</th>
                <th>类型 / 状态</th>
                <th>发货点</th>
                <th>集群 / 目的仓</th>
                <th>当前时段</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr
                  key={row.orderId}
                  className={
                    selected?.orderId === row.orderId ? "row-selected" : ""
                  }
                >
                  <td>
                    <b>{row.orderNumber || row.orderId}</b>
                    <small>{row.createdDate}</small>
                  </td>
                  <td>
                    <b>{row.supplyType}</b>
                    <small>
                      {row.state} · {row.supplyStates}
                    </small>
                  </td>
                  <td>
                    <b>{row.dropoffName || "—"}</b>
                    <small>{row.dropoffAddress}</small>
                  </td>
                  <td>
                    <b>{row.clusters || "—"}</b>
                    <small>
                      {row.storageWarehouses || `${row.suppliesCount} 个供应`}
                    </small>
                  </td>
                  <td>
                    <b>{row.timeslotFrom || "未预约"}</b>
                    <small>{row.timeslotTo}</small>
                  </td>
                  <td>
                    <button
                      className="outline-button"
                      onClick={() => findSlots(row)}
                    >
                      查询时段
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {!rows.length && !busy && (
            <div className="empty">
              当前没有活动供应单，或尚未配置 Seller API 权限。
            </div>
          )}
        </section>
        <aside className="card supply-slots">
          <h3>可用预约时段</h3>
          <div className="supply-dates">
            <label>
              开始
              <input
                type="date"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </label>
            <label>
              结束
              <input
                type="date"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </label>
          </div>
          {selected && (
            <p>
              供应单：<b>{selected.orderNumber || selected.orderId}</b>
            </p>
          )}
          {slots.map((slot) => (
            <button key={slot.from} onClick={() => book(slot)}>
              <span>{slot.from}</span>
              <small>至 {slot.to}</small>
              <b>预约</b>
            </button>
          ))}
          {!slots.length && (
            <div className="empty">选择供应单查询未来 60 天内的可用时段。</div>
          )}
          <p className="write-warning">
            预约是远程写操作，提交前必须手工输入“确认预约”；网络响应不明确时不会重复提交。
          </p>
        </aside>
      </div>
    </>
  );
}

export function SyncPage({ range }: { range: DateRange }) {
  const [logs, setLogs] = useState<SyncLog[]>([]),
    [coverage, setCoverage] = useState<DataCoverageRow[]>([]),
    [syncRange, setSyncRange] = useState<DateRange>(range),
    [pruneBefore, setPruneBefore] = useState(() => {
      const d = new Date();
      d.setDate(d.getDate() - 90);
      return d.toISOString().slice(0, 10);
    }),
    [busy, setBusy] = useState(""),
    [message, setMessage] = useState("");
  const today = new Date().toISOString().slice(0, 10),
    recentFrom = (days: number) =>
      new Date(Date.parse(`${today}T00:00:00Z`) - days * 86400000)
        .toISOString()
        .slice(0, 10),
    rangeValid =
      !!syncRange.from &&
      !!syncRange.to &&
      syncRange.from <= syncRange.to &&
      syncRange.to <= today;
  const load = async () => {
    const [l, c] = await Promise.all([syncLogs(), dataCoverage()]);
    setLogs(l);
    setCoverage(c);
  };
  useEffect(() => {
    void load();
  }, []);
  const seller = async () => {
    setBusy("seller");
    setMessage("");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    try {
      if (!rangeValid)
        throw new Error(
          "请选择有效的同步日期，结束日期不能早于开始日期或晚于今天。",
        );
      const count = await syncSeller(syncRange);
      clearReportCache();
      setMessage(`Seller 销量同步完成，写入 ${count} 行真实数据。`);
      await load();
    } catch (e) {
      setMessage(String(e));
      await load();
    } finally {
      setBusy("");
    }
  };
  const run = async (kind: "performance" | "finance") => {
    setBusy(kind);
    setMessage("");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    try {
      const count =
        kind === "performance"
          ? await syncPerformance(syncRange)
          : await syncFinance(syncRange);
      clearReportCache();
      setMessage(
        `${kind === "performance" ? "Performance 广告" : "Finance 结算"}同步完成，写入 ${count} 行。`,
      );
      await load();
    } catch (e) {
      setMessage(String(e));
      await load();
    } finally {
      setBusy("");
    }
  };
  const syncAll = async () => {
    if (!rangeValid) {
      setMessage("请选择有效的同步日期，结束日期不能早于开始日期或晚于今天。");
      return;
    }
    setBusy("all");
    setMessage("Seller、Performance 和 Finance 已通过三个后台线程同时开始同步…");
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      const result = await syncAllData(syncRange);
      clearReportCache();
      const items = [
        result.sellerError
          ? `Seller 失败：${result.sellerError}`
          : `Seller ${result.sellerRows ?? 0} 行`,
        result.performanceError
          ? `Performance 失败：${result.performanceError}`
          : `Performance ${result.performanceRows ?? 0} 行`,
        result.financeError
          ? `Finance 失败：${result.financeError}`
          : `Finance ${result.financeRows ?? 0} 行`,
      ];
      setMessage(`并行同步完成：${items.join("；")}`);
      await load();
    } catch (e) {
      setMessage(`并行同步启动失败：${String(e)}`);
      await load();
    } finally {
      setBusy("");
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">DATA SYNC CENTER</span>
          <h1>数据同步</h1>
          <p>API 数据写入当前店铺独立数据库，关闭软件后仍会保留</p>
        </div>
        <div className="sync-header-actions">
          <button
            className="sync-all-button"
            disabled={!!busy || !rangeValid}
            onClick={() => void syncAll()}
          >
            <RefreshCw className={busy === "all" ? "spin" : ""} size={16} />
            {busy === "all" ? "三线程同步中" : "同步所有数据"}
          </button>
          <button className="dark-button" disabled={!!busy} onClick={load}>
            <RefreshCw size={16} />
            刷新状态
          </button>
        </div>
      </header>
      <div className="month-toolbar sync-range-toolbar">
        <b>同步日期</b>
        <label>
          开始
          <input
            type="date"
            value={syncRange.from}
            max={syncRange.to || today}
            onChange={(e) =>
              setSyncRange((current) => ({ ...current, from: e.target.value }))
            }
          />
        </label>
        <em>至</em>
        <label>
          结束
          <input
            type="date"
            value={syncRange.to}
            min={syncRange.from}
            max={today}
            onChange={(e) =>
              setSyncRange((current) => ({ ...current, to: e.target.value }))
            }
          />
        </label>
        <button
          disabled={!!busy}
          onClick={() => setSyncRange({ from: recentFrom(6), to: today })}
        >
          最近 7 天
        </button>
        <button
          disabled={!!busy}
          onClick={() => setSyncRange({ from: recentFrom(29), to: today })}
        >
          最近 30 天
        </button>
        <button
          disabled={!!busy}
          onClick={() =>
            setSyncRange({ from: `${today.slice(0, 7)}-01`, to: today })
          }
        >
          本月
        </button>
      </div>
      {!rangeValid && (
        <div className="sync-message">
          请选择有效日期：结束日期不能早于开始日期，也不能晚于今天。
        </div>
      )}
      <div className="coverage-grid">
        {coverage.map((x) => (
          <section className="card" key={x.source}>
            <span>{x.source}</span>
            <b>{x.rowsCount.toLocaleString()} 行已缓存</b>
            <small>
              {x.dateFrom && x.dateTo
                ? `${x.dateFrom} 至 ${x.dateTo}`
                : "尚无本地数据"}
            </small>
            <small>
              {x.lastSuccess ? `最近成功：${x.lastSuccess}` : "尚无成功同步"}
            </small>
          </section>
        ))}
      </div>
      <div className="sync-actions">
        <section className="card">
          <Database />
          <div>
            <h3>Seller 销量分析</h3>
            <p>销售额、销量、妥投、退货、取消、浏览和加购</p>
            <small>
              本次请求：{syncRange.from} 至 {syncRange.to}
            </small>
          </div>
          <button
            className="dark-button"
            disabled={!!busy || !rangeValid}
            onClick={seller}
          >
            {busy === "seller" || busy === "all" ? "同步中" : "立即同步"}
          </button>
        </section>
        <section className="card">
          <Megaphone />
          <div>
            <h3>Performance 广告</h3>
            <p>活动日报、曝光、点击、订单和花费</p>
            <small>
              本次请求：{syncRange.from} 至 {syncRange.to}
            </small>
          </div>
          <button
            className="dark-button"
            disabled={!!busy || !rangeValid}
            onClick={() => run("performance")}
          >
            {busy === "performance" || busy === "all" ? "同步中" : "立即同步"}
          </button>
        </section>
        <section className="card">
          <TrendingUp />
          <div>
            <h3>Finance 结算</h3>
            <p>逐笔应计、佣金、配送和退货费用</p>
            <small>
              本次请求：{syncRange.from} 至 {syncRange.to}
            </small>
          </div>
          <button
            className="dark-button"
            disabled={!!busy || !rangeValid}
            onClick={() => run("finance")}
          >
            {busy === "finance" || busy === "all" ? "同步中" : "立即同步"}
          </button>
        </section>
      </div>
      {message && <div className="sync-message">{message}</div>}
      <div className="month-toolbar sync-range-toolbar">
        <b>历史缓存维护</b>
        <span>
          默认至少保留最近三个月；可删除此日期之前的销售、广告和 Finance 缓存
        </span>
        <input
          type="date"
          value={pruneBefore}
          max={new Date().toISOString().slice(0, 10)}
          onChange={(e) => setPruneBefore(e.target.value)}
        />
        <button
          disabled={!!busy}
          onClick={async () => {
            if (
              !window.confirm(
                `确认删除 ${pruneBefore} 之前的历史经营缓存？商品成本与配置不会删除。`,
              )
            )
              return;
            setBusy("prune");
            try {
              const count = await pruneCache(pruneBefore);
              clearReportCache();
              setMessage(
                `已清理 ${count.toLocaleString()} 条旧缓存，保留 ${pruneBefore} 及之后的数据。`,
              );
              await load();
            } catch (e) {
              setMessage(String(e));
            } finally {
              setBusy("");
            }
          }}
        >
          {busy === "prune" ? "清理中" : "清理所选日期前缓存"}
        </button>
      </div>
      <section className="card table-card">
        <div className="card-title">最近同步记录</div>
        <table>
          <thead>
            <tr>
              <th>开始时间</th>
              <th>来源</th>
              <th>状态</th>
              <th>行数</th>
              <th>完成时间</th>
              <th>消息</th>
            </tr>
          </thead>
          <tbody>
            {logs.map((log) => (
              <tr key={log.id}>
                <td>{log.startedAt}</td>
                <td>
                  <b>{log.source}</b>
                </td>
                <td>
                  <span className={`sync-status ${log.status}`}>
                    {log.status}
                  </span>
                </td>
                <td>{log.rowsCount}</td>
                <td>{log.finishedAt || "—"}</td>
                <td>{log.message}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  );
}

const matchesShipmentSettlement=(shipment:ShipmentTracking)=>["已送仓","已申请"].includes(shipment.cargoStatus.trim());

function ShipmentSkuEditor({ shipment, close, saved }: { shipment: ShipmentTracking; close: () => void; saved: () => void }) {
  const [options, setOptions] = useState<ShipmentSkuOption[]>([]);
  const [rows, setRows] = useState<ShipmentSkuAllocation[]>(shipment.skuAllocations.length ? shipment.skuAllocations : [{ sku: "", quantity: 1 }]);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { void shipmentSkuOptions(query).then(setOptions); }, [query]);
  const total = rows.reduce((sum, row) => sum + (Number(row.quantity) || 0), 0);
  return <div className="modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
    <div className="cost-modal shipment-sku-modal">
      <button className="modal-close" onClick={close}>×</button>
      <h2>配置批次 SKU</h2>
      <p><b>{shipment.trackingId}</b> · {shipment.productName || "未命名批次"} · 批次总数 {shipment.quantity} 件</p>
      <label>搜索 SKU / 货号 / 商品名<input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="例如 GJYB001" /></label>
      <div className="shipment-sku-rows">
        {rows.map((row, index) => <div className="shipment-sku-row" key={`${index}-${row.sku}`}>
          <select value={row.sku} onChange={(e) => setRows(rows.map((item, i) => i === index ? { ...item, sku: e.target.value } : item))}>
            <option value="">请选择产品 SKU</option>
            {options.map((option) => <option key={option.sku} value={option.sku}>{option.offerId || option.sku} · {option.sku} · {option.name}</option>)}
            {row.sku && !options.some((option) => option.sku === row.sku) && <option value={row.sku}>{row.sku}</option>}
          </select>
          <input type="number" min="1" step="1" value={row.quantity} onChange={(e) => setRows(rows.map((item, i) => i === index ? { ...item, quantity: Number(e.target.value) } : item))} />
          <button className="outline-button" onClick={() => setRows(rows.filter((_, i) => i !== index))}><Trash2 size={14} />删除</button>
        </div>)}
      </div>
      <button className="outline-button" onClick={() => setRows([...rows, { sku: "", quantity: 1 }])}><Plus size={14} />添加 SKU</button>
      <p className={total > shipment.quantity ? "missing-cost" : ""}>已分配 {total} / {shipment.quantity} 件；未分配 {Math.max(0, shipment.quantity - total)} 件</p>
      <div className="modal-actions">
        <button className="dark-button" disabled={busy || total > shipment.quantity || rows.some((row) => !row.sku || row.quantity <= 0)} onClick={async () => {
          setBusy(true);
          try { await saveShipmentSkuAllocations(shipment.trackingId, rows); saved(); }
          catch (e) { window.alert(`保存失败：${String(e)}`); }
          finally { setBusy(false); }
        }}>{busy ? "保存中…" : "保存 SKU 明细"}</button>
        <button className="outline-button" onClick={close}>取消</button>
      </div>
    </div>
  </div>;
}

function ShipmentSettlementEditor({ shipment, close, saved }: { shipment: ShipmentTracking; close: () => void; saved: () => void }) {
  const [items,setItems]=useState<ShipmentSettlementItem[]>([]),[busy,setBusy]=useState(false),[error,setError]=useState("");
  useEffect(()=>{void shipmentSettlement(shipment.trackingId).then(setItems).catch((e)=>setError(String(e)));},[shipment.trackingId]);
  const setValue=(index:number,key:keyof ShipmentSettlementItem,value:string)=>setItems(items.map((item,i)=>i===index?{...item,[key]:key==="note"?value:Math.max(0,Number(value)||0)}:item));
  return <div className="modal-backdrop"><div className="cost-modal shipment-sku-modal"><button className="modal-close" onClick={close}>×</button>
    <h2>审核批次交货并完结</h2><p><b>{shipment.trackingId}</b> · 飞书状态 {shipment.cargoStatus}。Ozon“已申请”为当前 SKU 汇总，仅供核对，请填写本批次实际去向。</p>
    {error&&<div className="sync-message">{error}</div>}
    <div className="settlement-grid settlement-head"><b>SKU / 批次数</b><b>当前已申请</b><b>本批次 FBO</b><b>转入 FBS</b><b>海外仓留存</b><b>短少/损耗</b><b>其他</b><b>备注</b></div>
    {items.map((item,index)=>{const total=item.fboQuantity+item.fbsQuantity+item.overseasRemainingQuantity+item.lossQuantity+item.otherQuantity;return <div className="settlement-grid" key={item.sku}>
      <span><b>{item.sku}</b><small>{item.batchQuantity} 件</small></span><span>{item.requestedStock}</span>
      {(["fboQuantity","fbsQuantity","overseasRemainingQuantity","lossQuantity","otherQuantity"] as const).map((key)=><input key={key} type="number" min="0" step="1" value={item[key]} onChange={(e)=>setValue(index,key,e.target.value)}/>)}
      <input value={item.note} onChange={(e)=>setValue(index,"note",e.target.value)} placeholder="差异原因"/>
      <small className={total===item.batchQuantity?"":"missing-cost"}>已处置 {total}/{item.batchQuantity}</small>
    </div>})}
    <div className="modal-actions"><button className="dark-button" disabled={busy||!items.length||items.some((x)=>x.fboQuantity+x.fbsQuantity+x.overseasRemainingQuantity+x.lossQuantity+x.otherQuantity!==x.batchQuantity)} onClick={async()=>{setBusy(true);try{await settleShipment(shipment.trackingId,items);saved();}catch(e){setError(String(e));}finally{setBusy(false);}}}>{busy?"确认中…":"确认平衡并完结批次"}</button><button className="outline-button" onClick={close}>取消</button></div>
  </div></div>;
}

export function FeishuPage({ range }: { range: DateRange }) {
  const [busy, setBusy] = useState(""),
    [result, setResult] = useState(""),
    [shipments, setShipments] = useState<ShipmentTracking[]>([]),
    [editingShipment, setEditingShipment] = useState<ShipmentTracking | null>(null),
    [settlingShipment, setSettlingShipment] = useState<ShipmentTracking | null>(null);
  const loadShipments = async () => setShipments(await shipmentTracking());
  useEffect(() => {
    void loadShipments();
  }, []);
  const run = async (kind: string, action: () => Promise<string>) => {
    setBusy(kind);
    setResult("");
    try {
      setResult(await action());
      await loadShipments();
    } catch (e) {
      setResult(String(e));
    } finally {
      setBusy("");
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">FEISHU COLLABORATION</span>
          <h1>飞书协作</h1>
          <p>商品成本双向同步、经营周报与发货跟踪，配置沿用旧版本地密钥</p>
        </div>
        <button
          className="dark-button"
          disabled={!!busy}
          onClick={() => run("test", testFeishu)}
        >
          <RefreshCw size={16} />
          {busy === "test" ? "测试中" : "测试连接"}
        </button>
      </header>
      <div className="feishu-grid">
        <section className="card">
          <Database />
          <h3>商品多维表格</h3>
          <p>
            以 SKU
            为唯一键同步货号、名称、采购成本、头程与备注。远程空值不会覆盖已有本地成本。
          </p>
          <div className="button-row">
            <button
              disabled={!!busy}
              onClick={() => run("pull", () => syncFeishuProducts("pull"))}
            >
              从飞书读取
            </button>
            <button
              disabled={!!busy}
              onClick={() => run("push", () => syncFeishuProducts("push"))}
            >
              推送到飞书
            </button>
            <button
              className="dark-button"
              disabled={!!busy}
              onClick={() => run("both", () => syncFeishuProducts("both"))}
            >
              双向同步
            </button>
          </div>
        </section>
        <section className="card">
          <TrendingUp />
          <h3>经营周报</h3>
          <p>
            使用当前店铺本地真实数据生成飞书互动卡片，包含销售、广告、退货取消与经营效率。
          </p>
          <small>
            {range.from} 至 {range.to}
          </small>
          <button
            className="dark-button"
            disabled={!!busy}
            onClick={() => run("weekly", () => sendFeishuWeekly(range))}
          >
            {busy === "weekly" ? "发送中" : "发送到群聊"}
          </button>
        </section>
      </div>
      {result && <div className="sync-message">{result}</div>}
      {editingShipment && <ShipmentSkuEditor shipment={editingShipment} close={() => setEditingShipment(null)} saved={() => { setEditingShipment(null); void loadShipments(); }} />}
      {settlingShipment && <ShipmentSettlementEditor shipment={settlingShipment} close={() => setSettlingShipment(null)} saved={() => { setSettlingShipment(null); void loadShipments(); }} />}
      <section className="card table-card shipment-card">
        <div className="card-title">
          发货跟踪{" "}
          <button
            className="outline-button"
            disabled={!!busy}
            onClick={() =>
              run(
                "shipments",
                async () =>
                  `发货跟踪同步完成，共读取 ${await syncFeishuShipments()} 条记录`,
              )
            }
          >
            <RefreshCw size={14} />
            从飞书同步
          </button>
        </div>
        <table>
          <thead>
            <tr>
              <th>跟踪单号</th>
              <th>品名 / 批次</th>
              <th>店铺 / 数量</th>
              <th>状态 / 渠道</th>
              <th>国内到库</th>
              <th>国外到库</th>
              <th>通知</th>
              <th>SKU 明细</th>
            </tr>
          </thead>
          <tbody>
            {shipments.map((row) => (
              <tr key={row.trackingId}>
                <td>
                  <b>{row.trackingId}</b>
                </td>
                <td>
                  {row.productName}
                  <small>{row.batchNo}</small>
                </td>
                <td>
                  {row.shopName}
                  <small>{row.quantity} 件</small>
                </td>
                <td>
                  {row.cargoStatus}
                  <small>{row.channel}</small>
                </td>
                <td>{row.domesticArrival || "—"}</td>
                <td>{row.foreignArrival || "—"}</td>
                <td>
                  {row.needsNotification ? (
                    <button
                      className="dark-button"
                      disabled={!!busy}
                      onClick={() =>
                        run(`notify-${row.trackingId}`, () =>
                          notifyShipment(row.trackingId),
                        )
                      }
                    >
                      发送到库通知
                    </button>
                  ) : (
                    <span className="badge green">
                      {row.notifiedForeignArrival ? "已通知" : "待到库"}
                    </span>
                  )}
                </td>
                <td>
                  <button className="outline-button" onClick={() => setEditingShipment(row)}>
                    <PackageSearch size={14} />{row.skuAllocations.length ? `${row.skuAllocations.length} 个 SKU` : "配置 SKU"}
                  </button>
                  {!!row.skuAllocations.length && <small>{row.skuAllocations.map((item) => `${item.sku} × ${item.quantity}`).join("；")}</small>}
                  {matchesShipmentSettlement(row) && <button className={row.settlementCompleted?"outline-button":"dark-button"} disabled={row.settlementCompleted} onClick={()=>setSettlingShipment(row)}>{row.settlementCompleted?"批次已完结":"审核差额并完结"}</button>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!shipments.length && (
          <div className="empty">
            尚无发货跟踪记录；配置跟踪表 Table ID 后点击“从飞书同步”。
          </div>
        )}
      </section>
      <section className="card migration-note">
        <AlertTriangle />
        <div>
          <h3>远程写入规则</h3>
          <p>
            以上按钮均由用户手动触发。商品同步按 SKU
            增量新增或更新；周报和到库通知不会在页面中自动发送。飞书 App Secret
            继续由 Windows DPAPI 加密保存。
          </p>
        </div>
      </section>
    </>
  );
}

export function WbPage({
  range,
  section,
  days,
  setDays,
}: {
  range: DateRange;
  section: "daily" | "costs" | "settings";
  days: number;
  setDays: (days: number) => void;
}) {
  const wbChartRef = useRef<HTMLDivElement | null>(null);
  const [tab, setTab] = useState<
      "daily" | "orders" | "ads" | "inventory" | "costs" | "settings"
    >(section),
    [settings, setSettings] = useState<WbSettings | null>(null),
    [rows, setRows] = useState<WbDaily[]>([]),
    [orderRows, setOrderRows] = useState<WbOrderRow[]>([]),
    [adRows, setAdRows] = useState<WbAdRow[]>([]),
    [warehouseRows, setWarehouseRows] = useState<WbWarehouseRow[]>([]),
    [stockRows, setStockRows] = useState<WbStockRow[]>([]),
    [costs, setCosts] = useState<WbCost[]>([]),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [wbApiPath, setWbApiPath] = useState("");
  const load = async () => {
    const [s, d, c, o, a, w, stocks] = await Promise.all([
      wbSettings(),
      wbDaily(range),
      wbCosts(),
      wbOrders(range),
      wbAds(range),
      wbWarehouses(),
      wbStocks(),
    ]);
    setSettings(s);
    setRows(d);
    setCosts(c);
    setOrderRows(o);
    setAdRows(a);
    setWarehouseRows(w);
    setStockRows(stocks);
  };
  useEffect(() => {
    void load();
  }, [range.from, range.to]);
  useEffect(() => setTab(section), [section]);
  useEffect(() => {
    if (tab !== "daily" || !wbChartRef.current) return;
    const daily = new Map<
      string,
      { revenue: number; ads: number; profit: number }
    >();
    rows.forEach((row) => {
      const item = daily.get(row.day) || { revenue: 0, ads: 0, profit: 0 };
      item.revenue += row.revenueCny;
      item.ads += row.adSpendCny;
      item.profit += row.profitCny || 0;
      daily.set(row.day, item);
    });
    const entries = [...daily.entries()].sort(([a], [b]) => a.localeCompare(b));
    const chart = echarts.init(wbChartRef.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { data: ["销售额", "广告费", "暂估利润"] },
      grid: { left: 20, right: 62, top: 52, bottom: 30, containLabel: true },
      xAxis: {
        type: "category",
        boundaryGap: false,
        data: entries.map(([day]) => day),
        axisTick: { show: false },
        axisLine: { show: false },
      },
      yAxis: [
        {
          type: "value",
          name: "销售 / 利润 CNY",
          splitLine: { lineStyle: { color: "#eef2f7" } },
        },
        { type: "value", name: "广告 CNY", splitLine: { show: false } },
      ],
      series: [
        {
          name: "销售额",
          type: "line",
          smooth: true,
          symbol: "none",
          data: entries.map(([, value]) => value.revenue),
          lineStyle: { color: "#3478f6", width: 3 },
          areaStyle: {
            color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(52,120,246,.22)" },
              { offset: 1, color: "rgba(52,120,246,0)" },
            ]),
          },
        },
        {
          name: "广告费",
          type: "line",
          smooth: true,
          symbol: "none",
          yAxisIndex: 1,
          data: entries.map(([, value]) => value.ads),
          lineStyle: { color: "#f39a32", width: 2 },
        },
        {
          name: "暂估利润",
          type: "line",
          smooth: true,
          symbol: "none",
          data: entries.map(([, value]) => value.profit),
          lineStyle: { color: "#84cc16", width: 2, type: "dashed" },
        },
      ],
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [rows, tab]);
  const total = (key: keyof WbDaily) =>
    rows.reduce(
      (n, r) => n + (typeof r[key] === "number" ? Number(r[key]) : 0),
      0,
    );
  const units = total("quantity"),
    revenue = total("revenueCny"),
    adSpend = total("adSpendCny"),
    adSales = adRows.reduce((n, row) => n + row.salesCny, 0),
    adOrders = adRows.reduce((n, row) => n + row.orders, 0),
    adViews = adRows.reduce((n, row) => n + row.views, 0),
    adClicks = adRows.reduce((n, row) => n + row.clicks, 0),
    activeProducts = new Set([
      ...rows.map((row) => row.nmId),
      ...stockRows.filter((row) => row.quantity > 0).map((row) => row.nmId),
    ]).size,
    avgOrder = units > 0 ? revenue / units : 0,
    acos = adSales > 0 ? (adSpend / adSales) * 100 : null,
    tacos = revenue > 0 ? (adSpend / revenue) * 100 : null,
    ctr = adViews > 0 ? (adClicks / adViews) * 100 : null,
    adConversion = adClicks > 0 ? (adOrders / adClicks) * 100 : null;
  const sync = async () => {
    setBusy(true);
    setMessage(
      "WB API 正在后台同步，当前页面仍可继续浏览；完成后会自动刷新缓存。",
    );
    try {
      setMessage(await syncWb(range));
      await load();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <header className="page-header wb-header">
        <div>
          <span className="eyebrow">WILDBERRIES WORKSPACE</span>
          <h1>WB 跨境独立工作台</h1>
          <p>
            独立 Token、独立数据库、精确商品广告与暂估利润，不混入 Ozon 数据
          </p>
        </div>
        <button className="dark-button" disabled={busy} onClick={sync}>
          <RefreshCw size={16} />
          {busy ? "同步中" : "同步 WB API"}
        </button>
      </header>
      <div className="tabs wb-tabs">
        <button
          className={tab === "daily" ? "selected" : ""}
          onClick={() => setTab("daily")}
        >
          每日经营
        </button>
        <button
          className={tab === "orders" ? "selected" : ""}
          onClick={() => setTab("orders")}
        >
          订单
        </button>
        <button
          className={tab === "ads" ? "selected" : ""}
          onClick={() => setTab("ads")}
        >
          广告
        </button>
        <button
          className={tab === "inventory" ? "selected" : ""}
          onClick={() => setTab("inventory")}
        >
          仓库与库存
        </button>
        <button
          className={tab === "costs" ? "selected" : ""}
          onClick={() => setTab("costs")}
        >
          产品成本与运费
        </button>
        <button
          className={tab === "settings" ? "selected" : ""}
          onClick={() => setTab("settings")}
        >
          WB API 设置
        </button>
      </div>
      {message && <div className="sync-message">{message}</div>}
      {tab === "daily" && (
        <>
          <div className="hero-grid">
            <section className="card summary">
              <div className="card-title">
                实时销量 <span className="badge green">WB 本地缓存</span>
              </div>
              <div className="four-cols">
                <div className="stat blue">
                  <span>销量</span>
                  <strong>{units}</strong>
                  <small>有效商品件数</small>
                </div>
                <div className="stat blue">
                  <span>销售额</span>
                  <strong>
                    ¥
                    {revenue.toLocaleString(undefined, {
                      maximumFractionDigits: 2,
                    })}
                  </strong>
                  <small>订单实收预估</small>
                </div>
                <div className="stat blue">
                  <span>订单商品行</span>
                  <strong>
                    {orderRows.filter((row) => !row.cancelled).length}
                  </strong>
                  <small>按 srid 去重</small>
                </div>
                <div className="stat blue">
                  <span>平均件单价</span>
                  <strong>¥{avgOrder.toFixed(2)}</strong>
                  <small>销售额 ÷ 件数</small>
                </div>
              </div>
            </section>
            <section className="card summary">
              <div className="card-title">
                广告表现 <span className="badge purple">商品级精确归因</span>
              </div>
              <div className="four-cols">
                <div className="stat purple">
                  <span>广告花费</span>
                  <strong>¥{adSpend.toFixed(2)}</strong>
                  <small>WB Promotion</small>
                </div>
                <div className="stat purple">
                  <span>广告销售额</span>
                  <strong>¥{adSales.toFixed(2)}</strong>
                  <small>归因销售</small>
                </div>
                <div className="stat purple">
                  <span>广告订单</span>
                  <strong>{adOrders}</strong>
                  <small>商品级缓存</small>
                </div>
                <div className="stat purple">
                  <span>点击转化率</span>
                  <strong>
                    {adConversion == null ? "—" : `${adConversion.toFixed(2)}%`}
                  </strong>
                  <small>广告订单 ÷ 点击</small>
                </div>
              </div>
            </section>
          </div>
          {!adRows.length && orderRows.length > 0 && (
            <div className="error-banner">
              <AlertTriangle />
              订单已有数据，但广告缓存是 0 行。WB 广告接口要求 Token
              开通“推广”权限；如果后台确有广告，请更新 Token
              权限后重新同步。系统不会用订单数据伪造广告。
            </div>
          )}
          <section className="card trend-card">
            <div className="section-heading">
              <div>
                <h2>业绩趋势</h2>
                <p>销售额、广告花费和暂估利润的日度变化</p>
              </div>
              <div className="trend-actions">
                <div className="tabs">
                  {(
                    [
                      [7, "最近7天"],
                      [30, "最近30天"],
                      [90, "本季度"],
                    ] as const
                  ).map(([value, label]) => (
                    <button
                      key={value}
                      className={days === value ? "selected" : ""}
                      onClick={() => setDays(value)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <div className="date-pill">
                  <CalendarDays size={16} />
                  {range.from} — {range.to}
                </div>
              </div>
            </div>
            <div className="metric-strip">
              <div className="stat blue selected-stat">
                <span>销售额</span>
                <strong>¥{revenue.toFixed(2)}</strong>
                <small>已汇总</small>
              </div>
              <div className="stat green">
                <span>销量</span>
                <strong>{units}</strong>
                <small>有效件数</small>
              </div>
              <div className="stat orange">
                <span>广告花费</span>
                <strong>¥{adSpend.toFixed(2)}</strong>
                <small>{adRows.length ? "商品级缓存" : "暂无缓存"}</small>
              </div>
              <div className="stat green">
                <span>在售商品</span>
                <strong>{activeProducts}</strong>
                <small>订单与库存快照</small>
              </div>
            </div>
            <div ref={wbChartRef} className="report-chart" />
          </section>
          <div className="health-grid">
            <section className="card health-card">
              <div className="card-title">
                经营健康度 <span className="badge blue">真实指标</span>
              </div>
              <div className="health-list">
                <span>
                  广告 ACOS<b>{acos == null ? "—" : `${acos.toFixed(2)}%`}</b>
                </span>
                <span>
                  整体 TACOS / DRR
                  <b>{tacos == null ? "—" : `${tacos.toFixed(2)}%`}</b>
                </span>
                <span>
                  广告 CTR<b>{ctr == null ? "—" : `${ctr.toFixed(2)}%`}</b>
                </span>
                <span>
                  点击－下单转化
                  <b>
                    {adConversion == null ? "—" : `${adConversion.toFixed(2)}%`}
                  </b>
                </span>
                <span>
                  取消商品行
                  <b>{orderRows.filter((row) => row.cancelled).length}</b>
                </span>
                <span>
                  缺成本 SKU<b>{rows.filter((row) => !row.complete).length}</b>
                </span>
              </div>
            </section>
            <section className="card sync">
              <div className="card-title">
                同步状态{" "}
                <span className={`badge ${busy ? "blue" : "green"}`}>
                  ● {busy ? "同步中" : "已连接"}
                </span>
              </div>
              <p>WB 数据按独立 Token 同步并保存至独立 SQLite。</p>
              <div>
                <span>
                  WB 连接<b>{settings?.token ? "已配置" : "未配置"}</b>
                </span>
                <span>
                  广告缓存<b>{adRows.length} 行</b>
                </span>
                <span>
                  数据来源<b>WB 本地快照</b>
                </span>
              </div>
            </section>
          </div>
          <div className="wb-weekly-action">
            <button
              className="outline-button"
              onClick={async () => {
                setBusy(true);
                try {
                  setMessage(await sendWbWeekly(range));
                } catch (e) {
                  setMessage(String(e));
                } finally {
                  setBusy(false);
                }
              }}
            >
              发送 WB 周利润到飞书
            </button>
          </div>
          <section className="card table-card">
            <table>
              <thead>
                <tr>
                  <th>日期 / nmId</th>
                  <th>货号 / 仓库</th>
                  <th>销量</th>
                  <th>销售额 CNY</th>
                  <th>广告</th>
                  <th>平台费</th>
                  <th>采购</th>
                  <th>物流</th>
                  <th>利润</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={`${row.day}-${row.nmId}`}>
                    <td>
                      {row.day}
                      <small>{row.nmId}</small>
                    </td>
                    <td>
                      <b>{row.article}</b>
                      <small>{row.warehouseName}</small>
                    </td>
                    <td>{row.quantity}</td>
                    <td>¥{row.revenueCny.toFixed(2)}</td>
                    <td>¥{row.adSpendCny.toFixed(2)}</td>
                    <td>¥{row.commissionCny.toFixed(2)}</td>
                    <td>
                      {row.purchaseTotalCny == null
                        ? "—"
                        : `¥${row.purchaseTotalCny.toFixed(2)}`}
                    </td>
                    <td>
                      {row.logisticsTotalCny == null
                        ? "—"
                        : `¥${row.logisticsTotalCny.toFixed(2)}`}
                    </td>
                    <td className={!row.complete ? "incomplete" : ""}>
                      {row.profitCny == null
                        ? "缺成本/物流"
                        : `¥${row.profitCny.toFixed(2)}`}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        </>
      )}
      {tab === "costs" && (
        <section className="card table-card">
          <table>
            <thead>
              <tr>
                <th>nmId</th>
                <th>货号</th>
                <th>采购 CNY</th>
                <th>长</th>
                <th>宽</th>
                <th>高</th>
                <th>重量 kg</th>
                <th>仓库模式</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {costs.map((row, i) => (
                <tr key={row.nmId}>
                  <td>{row.nmId}</td>
                  <td>
                    <input
                      value={row.article}
                      onChange={(e) =>
                        setCosts((x) =>
                          x.map((v, j) =>
                            j === i ? { ...v, article: e.target.value } : v,
                          ),
                        )
                      }
                    />
                  </td>
                  {(
                    [
                      "purchaseCostCny",
                      "lengthCm",
                      "widthCm",
                      "heightCm",
                      "weightKg",
                    ] as const
                  ).map((key) => (
                    <td key={key}>
                      <input
                        type="number"
                        value={row[key] ?? ""}
                        onChange={(e) =>
                          setCosts((x) =>
                            x.map((v, j) =>
                              j === i
                                ? {
                                    ...v,
                                    [key]:
                                      e.target.value === ""
                                        ? null
                                        : Number(e.target.value),
                                  }
                                : v,
                            ),
                          )
                        }
                      />
                    </td>
                  ))}
                  <td>
                    <select
                      value={row.warehouseMode}
                      onChange={(e) =>
                        setCosts((x) =>
                          x.map((v, j) =>
                            j === i
                              ? { ...v, warehouseMode: e.target.value }
                              : v,
                          ),
                        )
                      }
                    >
                      <option value="auto">自动识别</option>
                      <option value="dongguan">东莞跨境</option>
                      <option value="overseas">俄罗斯海外仓</option>
                      <option value="unknown">未知</option>
                    </select>
                  </td>
                  <td>
                    <button
                      onClick={async () => {
                        await saveWbCost(row);
                        setMessage(`已保存 ${row.nmId}`);
                        await load();
                      }}
                    >
                      <Save size={14} />
                      保存
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      {tab === "orders" && (
        <section className="card table-card">
          <div className="card-title">
            WB 实时订单缓存 <span>{orderRows.length} 行</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>日期 / srid</th>
                <th>nmId / 货号</th>
                <th>仓库</th>
                <th>金额 CNY</th>
                <th>状态</th>
                <th>更新时间</th>
              </tr>
            </thead>
            <tbody>
              {orderRows.slice(0, 1000).map((row) => (
                <tr key={row.srid}>
                  <td>
                    {row.day}
                    <small>{row.srid}</small>
                  </td>
                  <td>
                    {row.nmId}
                    <small>{row.article}</small>
                  </td>
                  <td>{row.warehouseName || "—"}</td>
                  <td>¥{row.revenueCny.toFixed(2)}</td>
                  <td>{row.cancelled ? "已取消" : "有效订单"}</td>
                  <td>{row.changedAt}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!orderRows.length && (
            <div className="empty">当前日期范围尚无 WB 订单缓存。</div>
          )}
        </section>
      )}
      {tab === "ads" && (
        <section className="card table-card">
          <div className="card-title">
            WB 商品级广告归因 <span>{adRows.length} 行</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>日期</th>
                <th>活动 / nmId</th>
                <th>曝光</th>
                <th>点击 / CTR</th>
                <th>广告订单</th>
                <th>花费 CNY</th>
                <th>广告销售 CNY</th>
              </tr>
            </thead>
            <tbody>
              {adRows.slice(0, 2000).map((row) => (
                <tr key={`${row.day}-${row.nmId}-${row.campaignId}`}>
                  <td>{row.day}</td>
                  <td>
                    {row.campaignId}
                    <small>{row.nmId}</small>
                  </td>
                  <td>{row.views}</td>
                  <td>
                    {row.clicks}
                    <small>
                      {row.ctr == null ? "—" : `${row.ctr.toFixed(2)}%`}
                    </small>
                  </td>
                  <td>{row.orders}</td>
                  <td>¥{row.spendCny.toFixed(2)}</td>
                  <td>¥{row.salesCny.toFixed(2)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!adRows.length && (
            <div className="empty">当前日期范围尚无商品级广告缓存。</div>
          )}
        </section>
      )}
      {tab === "inventory" && (
        <section className="card table-card">
          <div className="card-title">
            WB 仓库库存{" "}
            <span>
              {stockRows.length} 条库存 · {warehouseRows.length} 个目录仓库
            </span>
          </div>
          <div className="mini-stats">
            <div>
              <span>
                可用库存
                <strong>
                  {stockRows.reduce((n, row) => n + row.quantity, 0)} 件
                </strong>
              </span>
            </div>
            <div>
              <span>
                去客户途中
                <strong>
                  {stockRows.reduce((n, row) => n + row.inWayToClient, 0)} 件
                </strong>
              </span>
            </div>
            <div>
              <span>
                客户退回途中
                <strong>
                  {stockRows.reduce((n, row) => n + row.inWayFromClient, 0)} 件
                </strong>
              </span>
            </div>
          </div>
          <div className="migration-note">
            <AlertTriangle />
            <div>
              <h3>库存口径</h3>
              <p>
                数量来自 2026 新 Analytics `stocks-report/wb-warehouses`
                只读接口；仓库目录来自 Marketplace API。Token 无 Analytics
                权限时保留上次成功缓存，不回退到已停用接口。
              </p>
            </div>
          </div>
          <table>
            <thead>
              <tr>
                <th>nmId / chrtId</th>
                <th>仓库</th>
                <th>发货区域</th>
                <th>可用库存</th>
                <th>去客户途中</th>
                <th>退回途中</th>
                <th>更新时间</th>
              </tr>
            </thead>
            <tbody>
              {stockRows.slice(0, 5000).map((row) => (
                <tr key={`${row.nmId}-${row.chrtId}-${row.warehouseId}`}>
                  <td>
                    <b>{row.nmId}</b>
                    <small>{row.chrtId}</small>
                  </td>
                  <td>
                    {row.warehouseName}
                    <small>{row.warehouseId}</small>
                  </td>
                  <td>{row.regionName || "—"}</td>
                  <td>{row.quantity}</td>
                  <td>{row.inWayToClient}</td>
                  <td>{row.inWayFromClient}</td>
                  <td>{row.updatedAt}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!stockRows.length && (
            <div className="empty">
              尚无 WB 库存缓存；请使用具备 Analytics 权限的 Token 同步。
            </div>
          )}
          <div className="card-title">仓库目录</div>
          <table>
            <thead>
              <tr>
                <th>仓库</th>
                <th>地址</th>
                <th>城市</th>
                <th>国家</th>
                <th>物流识别</th>
                <th>来源键</th>
              </tr>
            </thead>
            <tbody>
              {warehouseRows.map((row) => (
                <tr key={row.warehouseKey}>
                  <td>
                    <b>{row.name || "—"}</b>
                  </td>
                  <td>{row.address || "—"}</td>
                  <td>{row.city || "—"}</td>
                  <td>{row.country || "—"}</td>
                  <td>{row.mode}</td>
                  <td>{row.warehouseKey}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {!warehouseRows.length && (
            <div className="empty">尚未同步 WB 仓库目录。</div>
          )}
        </section>
      )}
      {tab === "settings" && settings && (
        <section className="card wb-settings">
          <label>
            店铺名称
            <input
              value={settings.storeName}
              onChange={(e) =>
                setSettings({ ...settings, storeName: e.target.value })
              }
            />
          </label>
          <label>
            WB API Token
            <input
              type="password"
              value={settings.token}
              onChange={(e) =>
                setSettings({ ...settings, token: e.target.value })
              }
            />
          </label>
          <label>
            1 CNY = RUB
            <input
              type="number"
              value={settings.rubPerCny}
              onChange={(e) =>
                setSettings({ ...settings, rubPerCny: Number(e.target.value) })
              }
            />
          </label>
          <label>
            暂估平台费率 %
            <input
              type="number"
              value={settings.commissionPercent}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  commissionPercent: Number(e.target.value),
                })
              }
            />
          </label>
          <label>
            飞书 App ID
            <input
              value={settings.feishuAppId}
              onChange={(e) =>
                setSettings({ ...settings, feishuAppId: e.target.value })
              }
            />
          </label>
          <label>
            飞书 App Secret
            <input
              type="password"
              value={settings.feishuAppSecret}
              onChange={(e) =>
                setSettings({ ...settings, feishuAppSecret: e.target.value })
              }
            />
          </label>
          <label>
            飞书群 Chat ID
            <input
              value={settings.feishuChatId}
              onChange={(e) =>
                setSettings({ ...settings, feishuChatId: e.target.value })
              }
            />
          </label>
          <div className="button-row">
            <input
              value={wbApiPath}
              onChange={(e) => setWbApiPath(e.target.value)}
              placeholder="粘贴 .wb-api.json 完整路径"
            />
            <button
              onClick={async () =>
                setMessage(`WB API 已导出：${await exportWbApiBundle()}`)
              }
            >
              导出 WB API
            </button>
            <button
              disabled={!wbApiPath}
              onClick={async () => {
                await importWbApiBundle(wbApiPath);
                setMessage("WB API 配置已导入并用本机 DPAPI 重新加密");
                await load();
              }}
            >
              导入 WB API
            </button>
            <button onClick={async () => setMessage(await testWbFeishu())}>
              测试飞书
            </button>
            <button
              className="dark-button"
              onClick={async () => {
                await saveWbSettings(settings);
                setMessage("WB 设置已加密保存到独立数据库");
                await load();
              }}
            >
              <Save size={15} />
              保存 WB 设置
            </button>
          </div>
        </section>
      )}
    </>
  );
}

export function MigrationPage({ range }: { range: DateRange }) {
  const [path, setPath] = useState(""),
    [apiPath, setApiPath] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState("");
  const exports = [
    ...["sales", "销售日报"],
    ["orders", "订单明细"],
    ["advertising", "广告明细"],
    ["inventory", "库存快照"],
    ["finance", "Finance 流水"],
    ["costs", "Ozon 成本全字段"],
    ["products", "Ozon 产品与成本迁移包"],
    ["warehouses", "仓库集群映射"],
    ["shipments", "发货跟踪"],
    ["competitors", "竞品快照"],
    ["wb_costs", "WB 成本全字段"],
  ] as Array<[string, string]>;
  const run = async (kind: string) => {
    setBusy(kind);
    try {
      setMessage(`已导出：${await exportDataset(kind, range)}`);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy("");
    }
  };
  return (
    <>
      <header className="page-header">
        <div>
          <span className="eyebrow">DATA PORTABILITY</span>
          <h1>数据迁移与导出</h1>
          <p>
            将本地真实数据导出为带 UTF-8 BOM 的 CSV，便于换机、审计与二次处理
          </p>
        </div>
      </header>
      {message && <div className="sync-message">{message}</div>}
      <section className="migration-grid">
        {exports.map(([kind, label]) => (
          <button
            className="card export-tile"
            disabled={!!busy}
            onClick={() => run(kind)}
            key={kind}
          >
            <Database />
            <span>
              {label}
              <small>
                {kind === "costs" || kind === "wb_costs"
                  ? "包含成本、长宽高和重量"
                  : `${range.from} 至 ${range.to}`}
              </small>
            </span>
            <b>{busy === kind ? "导出中" : "导出 CSV"}</b>
          </button>
        ))}
      </section>
      <section className="card import-panel">
        <h3>API 配置导入与导出</h3>
        <p>
          与 Python 旧版 .ozon-api.json 完全兼容。导入时会在本机重新 DPAPI
          加密；导出文件含明文密钥，请妥善保管并在迁移后删除。
        </p>
        <div>
          <input
            value={apiPath}
            onChange={(e) => setApiPath(e.target.value)}
            placeholder="粘贴 .ozon-api.json 完整路径"
          />
          <button
            className="outline-button"
            disabled={!!busy}
            onClick={async () => {
              setBusy("api-export");
              try {
                setMessage(`API 配置已导出：${await exportApiBundle()}`);
              } catch (e) {
                setMessage(String(e));
              } finally {
                setBusy("");
              }
            }}
          >
            导出当前 API
          </button>
          <button
            className="dark-button"
            disabled={!apiPath || !!busy}
            onClick={async () => {
              setBusy("api-import");
              try {
                setMessage(
                  `成功导入 ${await importApiBundle(apiPath)} 组 API 配置`,
                );
              } catch (e) {
                setMessage(String(e));
              } finally {
                setBusy("");
              }
            }}
          >
            导入 API
          </button>
        </div>
      </section>
      <section className="card import-panel">
        <h3>导入 Ozon 产品 / 成本 CSV</h3>
        <p>
          兼容“成本全字段”和“产品与成本迁移包”：SKU、货号、商品
          ID、名称、图片、采购成本、头程、尺寸、重量和备注。已有 SKU
          按非空字段更新。
        </p>
        <div>
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="粘贴 CSV 文件完整路径，例如 D:\\迁移\\product_costs.csv"
          />
          <button
            className="dark-button"
            disabled={!path || !!busy}
            onClick={async () => {
              setBusy("import");
              try {
                setMessage(
                  `成功导入 ${await importProductCostsCsv(path)} 条产品成本`,
                );
              } catch (e) {
                setMessage(String(e));
              } finally {
                setBusy("");
              }
            }}
          >
            导入成本
          </button>
        </div>
      </section>
    </>
  );
}
