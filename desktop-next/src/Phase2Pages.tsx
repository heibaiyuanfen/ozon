import { useEffect, useState, type ReactNode } from "react";
import {
  Boxes,
  CheckCircle2,
  CircleAlert,
  Database,
  Download,
  KeyRound,
  PackageCheck,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  ShieldCheck,
  Store,
  Trash2,
  Warehouse,
  X,
} from "lucide-react";
import {
  createShop,
  deleteShop,
  exportProductCosts,
  loadCredentialsForm,
  matchProductCosts,
  saveCredentialsForm,
  saveProductCost,
  syncInventory,
  updateShop,
} from "./bridge";
import type {
  ConnectionStatus,
  CredentialsForm,
  InventoryRow,
  ProductCostInput,
  ProductRow,
  Shop,
} from "./types";

const money = (value: number, currency: string) =>
  `${currency === "CNY" ? "¥" : "₽"}${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`;

function PageTitle({
  eyebrow,
  title,
  subtitle,
}: {
  eyebrow: string;
  title: string;
  subtitle: string;
}) {
  return (
    <header className="page-header compact">
      <div>
        <span className="eyebrow">{eyebrow}</span>
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
    </header>
  );
}

function SearchBox({
  value,
  setValue,
  placeholder,
}: {
  value: string;
  setValue: (value: string) => void;
  placeholder: string;
}) {
  return (
    <label className="search">
      <Search size={16} />
      <input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        placeholder={placeholder}
      />
    </label>
  );
}

export function ProductsPage({
  rows,
  currency,
  query,
  setQuery,
  reload,
}: {
  rows: ProductRow[];
  currency: string;
  query: string;
  setQuery: (value: string) => void;
  reload: () => void;
}) {
  const [editing, setEditing] = useState<ProductRow | null>(null),
    [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(rows.length / 50)),
    visible = rows.slice(page * 50, page * 50 + 50);
  useEffect(() => setPage(0), [rows]);
  const revenue = rows.reduce((sum, row) => sum + row.revenue, 0),
    units = rows.reduce((sum, row) => sum + row.orderedUnits, 0),
    missing = rows.filter((row) => row.unitCost == null).length;
  const exportCosts = async () => {
    const path = await exportProductCosts();
    window.alert(`成本数据已导出：\n${path}`);
  };
  return (
    <>
      <PageTitle
        eyebrow="PRODUCT OPERATIONS"
        title="商品中心"
        subtitle="商品目录、销售表现与成本完整度"
      />
      <div className="mini-stats">
        <div>
          <Boxes />
          <span>
            商品数量<strong>{rows.length}</strong>
          </span>
        </div>
        <div>
          <PackageCheck />
          <span>
            周期销量<strong>{units} 件</strong>
          </span>
        </div>
        <div>
          <Database />
          <span>
            周期销售额<strong>{money(revenue, currency)}</strong>
          </span>
        </div>
        <div className={missing ? "warn" : ""}>
          <CircleAlert />
          <span>
            缺失成本<strong>{missing}</strong>
          </span>
        </div>
      </div>
      <div className="list-toolbar">
        <div>
          <b>商品表现</b>
          <small>按销售额降序，最多展示 2000 个商品</small>
        </div>
        <div className="toolbar-actions">
          <button className="outline-button" onClick={exportCosts}>
            <Download size={15} />
            导出成本
          </button>
          <SearchBox
            value={query}
            setValue={setQuery}
            placeholder="搜索货号或 SKU"
          />
        </div>
      </div>
      <section className="card table-card">
        <table>
          <thead>
            <tr>
              <th>货号 / Ozon SKU</th>
              <th>下单</th>
              <th>妥投</th>
              <th>退货</th>
              <th>取消</th>
              <th>销售额</th>
              <th>采购 CNY / 头程 RUB</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => (
              <tr key={row.sku}>
                <td>
                  <b>{row.offerId || "—"}</b>
                  <small>{row.sku}</small>
                </td>
                <td>
                  <b>{row.orderedUnits}</b>
                </td>
                <td>{row.deliveredUnits}</td>
                <td>{row.returns}</td>
                <td>{row.cancellations}</td>
                <td>
                  <b>{money(row.revenue, currency)}</b>
                </td>
                <td>
                  {row.unitCost == null ? (
                    <span className="missing">待维护</span>
                  ) : (
                    <>
                      <b>
                        ¥
                        {row.unitCost.toLocaleString("zh-CN", {
                          maximumFractionDigits: 2,
                        })}
                      </b>
                      <small>
                        {row.firstMileCost == null
                          ? "头程未维护"
                          : `头程 ₽${row.firstMileCost.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`}
                      </small>
                    </>
                  )}
                </td>
                <td>
                  <button
                    className="icon-button"
                    onClick={() => setEditing(row)}
                  >
                    <Pencil size={15} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!rows.length && <div className="empty">当前条件下没有商品</div>}
      </section>
      <div className="table-pagination">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>
          上一页
        </button>
        <span>
          第 {page + 1} / {pages} 页 · 每页 50 条
        </span>
        <button disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>
          下一页
        </button>
      </div>
      {editing && (
        <CostEditor
          row={editing}
          close={() => setEditing(null)}
          saved={() => {
            setEditing(null);
            reload();
          }}
        />
      )}
    </>
  );
}

function numberValue(value: string): number | null {
  return value.trim() === "" ? null : Number(value);
}
function CostEditor({
  row,
  close,
  saved,
}: {
  row: ProductRow;
  close: () => void;
  saved: () => void;
}) {
  const [form, setForm] = useState({
    unitCost: row.unitCost?.toString() ?? "",
    firstMileCost: row.firstMileCost?.toString() ?? "",
    lengthCm: row.lengthCm?.toString() ?? "",
    widthCm: row.widthCm?.toString() ?? "",
    heightCm: row.heightCm?.toString() ?? "",
    weightKg: row.weightKg?.toString() ?? "",
    note: row.note,
    pattern: row.offerId.replace(/[-_].*$/, "") || row.offerId,
  });
  const [busy, setBusy] = useState(false);
  const input: ProductCostInput = {
    sku: row.sku,
    unitCost: numberValue(form.unitCost),
    firstMileCost: numberValue(form.firstMileCost),
    lengthCm: numberValue(form.lengthCm),
    widthCm: numberValue(form.widthCm),
    heightCm: numberValue(form.heightCm),
    weightKg: numberValue(form.weightKg),
    note: form.note,
  };
  const save = async (match: boolean) => {
    setBusy(true);
    try {
      if (match) {
        const count = await matchProductCosts(input, form.pattern);
        window.alert(`已统一匹配 ${count} 个 SKU 的成本、长宽高和重量`);
      } else await saveProductCost(input);
      saved();
    } finally {
      setBusy(false);
    }
  };
  const field = (key: keyof typeof form, label: string) => (
    <label>
      {label}
      <input
        value={form[key]}
        onChange={(e) => setForm({ ...form, [key]: e.target.value })}
      />
    </label>
  );
  return (
    <div className="modal-backdrop">
      <div className="cost-modal">
        <button className="modal-close" onClick={close}>
          <X />
        </button>
        <h2>
          {row.offerId} / {row.sku}
        </h2>
        <p>采购成本、头程与产品参数</p>
        <div className="cost-fields">
          {field("unitCost", "采购成本（CNY）")}
          {field("firstMileCost", "头程（RUB）")}
          {field("lengthCm", "长度（cm）")}
          {field("widthCm", "宽度（cm）")}
          {field("heightCm", "高度（cm）")}
          {field("weightKg", "重量（kg）")}
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
            onClick={() => save(false)}
          >
            保存此 SKU
          </button>
          <label>
            系列匹配
            <input
              value={form.pattern}
              onChange={(e) => setForm({ ...form, pattern: e.target.value })}
            />
          </label>
          <button
            className="outline-button"
            disabled={busy || !form.pattern}
            onClick={() => save(true)}
          >
            统一匹配全部参数
          </button>
        </div>
      </div>
    </div>
  );
}

export function InventoryPage({
  rows,
  query,
  setQuery,
  targetDays,
  setTargetDays,
  reload,
}: {
  rows: InventoryRow[];
  query: string;
  setQuery: (value: string) => void;
  targetDays: number;
  setTargetDays: (value: number) => void;
  reload: () => Promise<void>;
}) {
  const [syncing, setSyncing] = useState(false),
    [message, setMessage] = useState(""),
    [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(rows.length / 50)),
    visible = rows.slice(page * 50, page * 50 + 50);
  useEffect(() => setPage(0), [rows]);
  const sync = async () => {
    setSyncing(true);
    setMessage("");
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    try {
      const count = await syncInventory();
      setMessage(`库存同步完成，写入 ${count} 条仓库库存记录。`);
      await reload();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setSyncing(false);
    }
  };
  const available = rows.reduce((s, x) => s + x.availableStock, 0),
    portal = rows.reduce((s, x) => s + (x.portalStock ?? x.availableStock), 0),
    transit = rows.reduce((s, x) => s + x.transitStock, 0),
    suggested = rows.reduce((s, x) => s + x.suggestedQty, 0);
  return (
    <>
      <PageTitle
        eyebrow="INVENTORY CONTROL"
        title="库存管理"
        subtitle="可售、在途、已申请与建议补货的本地快照"
      />
      <div className="inventory-sync-bar">
        <span>
          {message || "库存同步在后台执行，期间可以继续浏览其他页面。"}
        </span>
        <button className="dark-button" disabled={syncing} onClick={sync}>
          <RefreshCw size={15} className={syncing ? "spin" : ""} />
          {syncing ? "正在同步库存" : "同步全部库存"}
        </button>
      </div>
      <div className="mini-stats">
        <div>
          <Warehouse />
          <span>
            后台总库存<strong>{portal} 件</strong>
          </span>
        </div>
        <div>
          <PackageCheck />
          <span>
            在途库存<strong>{transit} 件</strong>
          </span>
        </div>
        <div>
            <Boxes />
          <span>
            可售库存<strong>{available} 件</strong>
          </span>
        </div>
        <div className={suggested ? "warn" : ""}>
          <CircleAlert />
          <span>
            建议补货<strong>{suggested} 件</strong>
          </span>
        </div>
      </div>
      <div className="list-toolbar">
        <div>
          <b>库存明细</b>
          <small>
            建议量沿用：MAX(0, 日均销量 × 目标天数 − 可售 − 在途 − 已申请)
          </small>
        </div>
        <div className="toolbar-actions">
          <label className="days-input">
            目标天数{" "}
            <input
              type="number"
              min="1"
              max="365"
              value={targetDays}
              onChange={(e) =>
                setTargetDays(Math.max(1, Number(e.target.value) || 30))
              }
            />
          </label>
          <SearchBox
            value={query}
            setValue={setQuery}
            placeholder="搜索货号或 SKU"
          />
        </div>
      </div>
      <section className="card table-card">
        <table>
          <thead>
            <tr>
              <th>货号 / SKU</th>
              <th>后台总库存</th>
              <th>可售</th>
              <th>预留</th>
              <th>在途</th>
              <th>已申请</th>
              <th>仓库</th>
              <th>日均销量</th>
              <th>预计可售</th>
              <th>建议 / 计划</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => (
              <tr key={row.sku}>
                <td>
                  <b>{row.offerId || "—"}</b>
                  <small>{row.sku}</small>
                </td>
                <td><b>{row.portalStock ?? "—"}</b></td>
                <td>
                  <b>{row.availableStock}</b>
                </td>
                <td>{row.reservedStock ?? "—"}</td>
                <td>{row.transitStock}</td>
                <td>{row.requestedStock}</td>
                <td>{row.warehouseCount}</td>
                <td>{row.dailySales.toFixed(1)}</td>
                <td>
                  {row.estimatedDays == null
                    ? "—"
                    : `${row.estimatedDays.toFixed(1)} 天`}
                </td>
                <td>
                  <b className={row.suggestedQty > 0 ? "danger-text" : ""}>
                    {row.suggestedQty}
                  </b>
                  <small>计划 {row.plannedQty}</small>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!rows.length && <div className="empty">尚未同步库存快照</div>}
      </section>
      <div className="table-pagination">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>
          上一页
        </button>
        <span>
          第 {page + 1} / {pages} 页 · 每页 50 条
        </span>
        <button disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>
          下一页
        </button>
      </div>
    </>
  );
}

export function ShopsPage({
  shops,
  activeShop,
  select,
  reload,
}: {
  shops: Shop[];
  activeShop?: Shop;
  select: (id: string) => void;
  reload: () => Promise<void>;
}) {
  const [editing, setEditing] = useState<Shop | null>(null),
    [creating, setCreating] = useState(false),
    [form, setForm] = useState({
      name: "",
      kind: "local" as "local" | "cross_border",
      apiName: "",
    }),
    [message, setMessage] = useState("");
  const openEdit = (shop: Shop) => {
    setEditing(shop);
    setCreating(false);
    setForm({ name: shop.name, kind: shop.kind, apiName: shop.apiName });
  };
  const save = async () => {
    if (creating) await createShop(form.name, form.kind, form.apiName);
    else if (editing)
      await updateShop(editing.id, form.name, form.kind, form.apiName);
    setEditing(null);
    setCreating(false);
    setMessage("店铺注册表已保存");
    await reload();
  };
  return (
    <>
      <PageTitle
        eyebrow="STORE MANAGEMENT"
        title="店铺管理"
        subtitle="店铺切换、新增、重命名、类型与独立数据库"
      />
      <div className="shop-toolbar">
        <button
          className="dark-button"
          onClick={() => {
            setCreating(true);
            setEditing(null);
            setForm({ name: "", kind: "local", apiName: "" });
          }}
        >
          <Plus size={15} />
          新增店铺
        </button>
      </div>
      <div className="shop-grid">
        {shops.map((shop) => (
          <article
            key={shop.id}
            className={`shop-card ${shop.id === activeShop?.id ? "active" : ""}`}
          >
            <button className="shop-main" onClick={() => select(shop.id)}>
              <div className="shop-icon">
                <Store />
              </div>
              <span className={`kind ${shop.kind}`}>
                {shop.kind === "cross_border" ? "跨境店" : "本土店"}
              </span>
              <h3>{shop.name}</h3>
              <p>{shop.apiName || "未命名 API 配置"}</p>
              <small>
                <Database size={13} /> 独立 SQLite 数据库
              </small>
              {shop.id === activeShop?.id && (
                <b className="current">
                  <CheckCircle2 size={14} /> 当前店铺
                </b>
              )}
            </button>
            <div className="shop-actions">
              <button onClick={() => openEdit(shop)}>
                <Pencil size={14} />
                编辑
              </button>
              <button
                disabled={shop.id === activeShop?.id || shops.length <= 1}
                onClick={async () => {
                  const confirmation = window.prompt(
                    `删除后数据库会移动到 data/trash，可恢复。\n请输入店铺名称“${shop.name}”确认：`,
                  );
                  if (confirmation == null) return;
                  try {
                    setMessage(await deleteShop(shop.id, confirmation));
                    await reload();
                  } catch (e) {
                    setMessage(String(e));
                  }
                }}
              >
                <Trash2 size={14} />
                删除
              </button>
            </div>
          </article>
        ))}
      </div>
      {(editing || creating) && (
        <div className="modal-backdrop">
          <div className="cost-modal shop-modal">
            <button
              className="modal-close"
              onClick={() => {
                setEditing(null);
                setCreating(false);
              }}
            >
              <X />
            </button>
            <h2>{creating ? "新增店铺" : "编辑店铺"}</h2>
            <div className="cost-fields">
              <label>
                店铺名称
                <input
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </label>
              <label>
                店铺类型
                <select
                  value={form.kind}
                  onChange={(e) =>
                    setForm({
                      ...form,
                      kind: e.target.value as "local" | "cross_border",
                    })
                  }
                >
                  <option value="local">本土店</option>
                  <option value="cross_border">跨境店</option>
                </select>
              </label>
              <label className="wide">
                API 配置名称
                <input
                  value={form.apiName}
                  onChange={(e) =>
                    setForm({ ...form, apiName: e.target.value })
                  }
                />
              </label>
            </div>
            <button
              className="dark-button"
              disabled={!form.name.trim()}
              onClick={save}
            >
              保存店铺
            </button>
          </div>
        </div>
      )}
      {message && <div className="sync-message">{message}</div>}
      <section className="card migration-note">
        <ShieldCheck />
        <div>
          <h3>数据隔离与安全删除</h3>
          <p>
            每个店铺继续使用独立 SQLite
            数据库。新增店铺只复制数据库结构，不复制业务数据；删除时数据库移动到
            data/trash，不会永久擦除。
          </p>
        </div>
      </section>
    </>
  );
}

function ConnectionCard({
  icon,
  title,
  configured,
  lines,
}: {
  icon: ReactNode;
  title: string;
  configured: boolean;
  lines: string[];
}) {
  return (
    <div className="connection-card">
      <div className="connection-head">
        <span>{icon}</span>
        <div>
          <h3>{title}</h3>
          <small className={configured ? "ok" : "pending"}>
            {configured ? "● 已配置" : "○ 待配置"}
          </small>
        </div>
      </div>
      {lines.map((line, i) => (
        <p key={i}>{line || "—"}</p>
      ))}
      <div className="secret-note">
        <KeyRound size={13} /> 密钥仅保存在 Windows 加密存储中
      </div>
    </div>
  );
}
export function SettingsPage({ status }: { status: ConnectionStatus | null }) {
  const [form, setForm] = useState<CredentialsForm | null>(null),
    [message, setMessage] = useState("");
  useEffect(() => {
    loadCredentialsForm().then(setForm);
  }, []);
  if (!status) return <div className="empty">正在读取连接状态…</div>;
  const field = (key: keyof CredentialsForm, label: string, secret = false) => (
    <label>
      {label}
      <input
        type={secret ? "password" : "text"}
        value={form?.[key] ?? ""}
        placeholder={secret ? "留空则保留现有密钥" : ""}
        onChange={(e) => form && setForm({ ...form, [key]: e.target.value })}
      />
    </label>
  );
  const save = async () => {
    if (!form) return;
    try {
      await saveCredentialsForm(form);
      window.dispatchEvent(new Event("report-settings-changed"));
      setMessage(
        "配置已保存；汇率、税率和回款手续费已清除旧报表缓存并将按新参数计算。",
      );
    } catch (error) {
      setMessage(String(error));
    }
  };
  return (
    <>
      <PageTitle
        eyebrow="CONNECTION SETTINGS"
        title="连接设置"
        subtitle="兼容旧版 DPAPI 密文，密钥不会回显到界面"
      />
      <div className="connection-grid">
        <ConnectionCard
          icon={<Server />}
          title="Ozon Seller API"
          configured={status.sellerApiConfigured}
          lines={[
            `Client ID：${status.sellerClientId || "未填写"}`,
            `最近同步：${status.lastSuccessfulSync || "尚未同步"}`,
          ]}
        />
        <ConnectionCard
          icon={<Database />}
          title="Performance API"
          configured={status.performanceApiConfigured}
          lines={[
            `Client ID：${status.performanceClientId || "未填写"}`,
            "用于广告活动与日报同步",
          ]}
        />
        <ConnectionCard
          icon={<ShieldCheck />}
          title="AI 分析"
          configured={status.aiConfigured}
          lines={[
            `服务地址：${status.aiBaseUrl || "未填写"}`,
            `模型：${status.aiModel || "未填写"}`,
          ]}
        />
        <ConnectionCard
          icon={<CheckCircle2 />}
          title="飞书同步"
          configured={status.feishuConfigured}
          lines={[
            "多维表格与机器人状态",
            status.feishuConfigured ? "应用凭证已保存" : "尚未保存完整凭证",
          ]}
        />
      </div>
      {form && (
        <section className="card credentials-editor">
          <h2>编辑连接凭证</h2>
          <p>密钥输入框为空时保留数据库内的原 DPAPI 密文。</p>
          <div className="credentials-section">
            <h3>Ozon API</h3>
            {field("sellerClientId", "Seller Client ID")}
            {field("sellerApiKey", "Seller API Key", true)}
            {field("performanceClientId", "Performance Client ID")}
            {field(
              "performanceClientSecret",
              "Performance Client Secret",
              true,
            )}
          </div>
          <div className="credentials-section">
            <h3>汇率与月度盈亏参数</h3>
            <p>
              Seller 销售、广告与 Finance 原始金额均为卢布。跨境页面按“卢布 ÷
              RUB/CNY”显示人民币；本土店采购成本按“人民币 ×
              RUB/CNY”计入卢布成本。
            </p>
            {field("localRubPerCny", "本土店汇率：1 CNY = RUB")}
            {field("crossBorderRubPerCny", "跨境店汇率：1 CNY = RUB")}
            {field("localTaxRate", "税率 %")}
            {field("localPayoutFeeRate", "回款手续费 %")}
          </div>
          <div className="credentials-section">
            <h3>AI</h3>
            {field("aiBaseUrl", "API Base URL")}
            {field("aiModel", "模型")}
            {field("aiApiKey", "API Key", true)}
          </div>
          <div className="credentials-section">
            <h3>飞书</h3>
            {field("feishuBaseUrl", "开放平台地址")}
            {field("feishuAppId", "App ID")}
            {field("feishuAppSecret", "App Secret", true)}
            {field("feishuAppToken", "App Token")}
            {field("feishuProductTableId", "商品表 Table ID")}
            {field("feishuWeeklyTableId", "周报表 Table ID")}
            {field("feishuTrackingTableId", "物流表 Table ID")}
            {field("feishuSeriesTableId", "系列表 Table ID")}
            {field("feishuChatId", "群 Chat ID")}
          </div>
          <div className="credentials-save">
            <button className="dark-button" onClick={save}>
              <KeyRound size={15} />
              安全保存
            </button>
            <span>{message}</span>
          </div>
        </section>
      )}
    </>
  );
}
