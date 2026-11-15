import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./procurement-board.css";
const stages = [
  "采购单",
  "采购合同",
  "付款",
  "工厂发货 / 装箱",
  "到物流仓",
  "发往海外",
  "海外到仓",
  "约仓",
  "送仓完成",
];
type Step = { status: string; plan: string; date: string; note: string };
type Doc = {
  id: string;
  name: string;
  stage: number;
  kind: string;
  data: string;
  cells?: string[][];
};
type Chain = {
  id: string;
  revision: number;
  orderNo: string;
  title: string;
  supplier: string;
  owner: string;
  invoice: string;
  invoiceDate: string;
  steps: Step[];
  docs: Doc[];
};
const fresh = (orderNo = ""): Chain => ({
  id: crypto.randomUUID(),
  revision: 0,
  orderNo,
  title: "",
  supplier: "",
  owner: "",
  invoice: "未开票",
  invoiceDate: "",
  steps: stages.map(() => ({ status: "待处理", plan: "", date: "", note: "" })),
  docs: [],
});
const today = () => new Date().toLocaleDateString("sv-SE");
const overdue = (c: Chain) =>
  c.steps.some(
    (s) =>
      s.plan &&
      s.plan < today() &&
      s.status !== "已完成" &&
      s.status !== "不适用",
  );
const closed = (c: Chain) =>
  c.steps.every((s) => ["已完成", "不适用"].includes(s.status)) &&
  ["已开票", "无需开票"].includes(c.invoice);
let pendingDraft: Chain | null = null;
export function ProcurementBoard() {
  const [rows, setRows] = useState<Chain[]>([]),
    [draft, setDraft] = useState<Chain | null>(() => pendingDraft),
    [dirty, setDirty] = useState(!!pendingDraft),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [query, setQuery] = useState(""),
    [filter, setFilter] = useState("全部"),
    [stage, setStage] = useState(0),
    [name, setName] = useState(""),
    [url, setUrl] = useState(""),
    [kind, setKind] = useState("表格"),
    [sources, setSources] = useState<
      { id: number; orderNo: string; title: string; operator: string }[]
    >([]);
  const load = async () =>
    setRows(
      await invoke<Chain[]>("procurement_board_command", {
        command: "list",
        payload: null,
      }),
    );
  useEffect(() => {
    void load().catch((e) => setMessage(String(e)));
    void invoke<{ orders: typeof sources }>("purchase_order_command", {
      command: "list",
      id: null,
      payload: null,
    })
      .then((x) => setSources(x.orders))
      .catch((e) => setMessage(`采购单读取失败：${e}`));
  }, []);
  useEffect(() => {
    const f = (e: BeforeUnloadEvent) => {
      if (dirty) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", f);
    return () => window.removeEventListener("beforeunload", f);
  }, [dirty]);
  useEffect(() => { pendingDraft = dirty ? draft : null; }, [draft, dirty]);
  const edit = (patch: Partial<Chain>) => {
    setDraft((c) => (c ? { ...c, ...patch } : c));
    setDirty(true);
  };
  const choose = (c: Chain) => { if (busy) return;
    if (dirty && !confirm("放弃尚未保存的修改？")) return;
    setDraft(structuredClone(c));
    setDirty(false);
    setMessage("");
  };
  const save = async () => {
    if (!draft || busy) return;
    setBusy(true);
    try {
      const r = await invoke<{ revision: number }>(
        "procurement_board_command",
        { command: "save", payload: draft },
      );
      setDraft({ ...draft, revision: r.revision });
      setDirty(false);
      await load();
      setMessage("已保存到本机统筹看板");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  const addFiles = async (files: FileList | null) => {
    if (!draft || !files) return;
    setBusy(true);
    try {
      const docs: Doc[] = [];
      for (const f of Array.from(files)) {
        if (f.size > 10 * 1024 * 1024) throw Error("单文件最大 10 MB");
        const data = await new Promise<string>((resolve, reject) => {
          const r = new FileReader();
          r.onload = () => resolve(String(r.result));
          r.onerror = () => reject(Error("文件读取失败"));
          r.readAsDataURL(f);
        });
        docs.push({ id: crypto.randomUUID(), name: f.name, stage, kind, data });
      }
      edit({ docs: [...draft.docs, ...docs] });
      setMessage("文件已加入，点击保存链路后持久保存");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  const addLink = () => {
    if (!draft) return;
    try {
      const u = new URL(url);
      if (!["https:", "http:"].includes(u.protocol)) throw Error();
      if (!name.trim()) throw Error();
      edit({
        docs: [
          ...draft.docs,
          {
            id: crypto.randomUUID(),
            name: name.trim(),
            stage,
            kind,
            data: u.href,
          },
        ],
      });
      setName("");
      setUrl("");
    } catch {
      setMessage("请填写资料名称及完整 http/https 链接");
    }
  };
  const shown = rows.filter(
    (c) =>
      (c.orderNo + " " + c.title + " " + c.supplier + " " + c.owner)
        .toLowerCase()
        .includes(query.toLowerCase()) &&
      (filter === "全部" ||
        (filter === "逾期" && overdue(c)) ||
        (filter === "未付款" &&
          c.steps[2].status !== "已完成" &&
          c.steps[2].status !== "不适用") ||
        (filter === "未开票" &&
          c.invoice !== "已开票" &&
          c.invoice !== "无需开票") ||
        (filter === "已闭环" && closed(c))),
  );
  return (
    <div className="pb">
      <header>
        <div>
          <small>PROCUREMENT · 全链路统筹</small>
          <h1>采购履约看板</h1>
          <p>按采购单号串联资料、款项与物流进度 · 全店共用 · 手动确认状态</p>
        </div>
        <button
          onClick={() => {
            choose(fresh());
            setDirty(true);
          }}
        >
          ＋ 新建链路
        </button>
      </header>
      <div className="pb-stats">
        {[
          ["全部", rows.length],
          ["逾期", rows.filter(overdue).length],
          [
            "未付款",
            rows.filter(
              (c) => !["已完成", "不适用"].includes(c.steps[2].status),
            ).length,
          ],
          [
            "未开票",
            rows.filter((c) => !["已开票", "无需开票"].includes(c.invoice))
              .length,
          ],
          ["已闭环", rows.filter(closed).length],
        ].map(([label, n]) => (
          <button
            key={label}
            className={filter === label ? "active" : ""}
            onClick={() => setFilter(String(label))}
          >
            <span>{label}</span>
            <strong>{n}</strong>
          </button>
        ))}
      </div>
      <div className="pb-tools">
        <input
          aria-label="搜索采购链路"
          placeholder="搜索采购单号 / 产品 / 供应商 / 负责人"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select
          value=""
          onChange={(e) => {
            const s = sources.find((s) => s.id === Number(e.target.value));
            if (s) {
              const existing = rows.find((c) => c.orderNo === s.orderNo);
              choose(
                existing || {
                  ...fresh(s.orderNo),
                  title: s.title,
                  owner: s.operator,
                },
              );
              if (!existing) setDirty(true);
            }
          }}
        >
          <option value="">从当前店铺采购单建立链路…</option>
          {sources.map((s) => (
            <option key={s.id} value={s.id}>
              {s.orderNo} · {s.title}
            </option>
          ))}
        </select>
        <button onClick={() => void load().catch((e) => setMessage(String(e)))}>
          刷新列表
        </button>
      </div>
      {message && (
        <div role="status" className="pb-message">
          {message}
        </div>
      )}
      <div className="pb-table">
        <table>
          <thead>
            <tr>
              <th>采购单 / 产品</th>
              <th>供应商 / 负责人</th>
              {stages.slice(1).map((s) => (
                <th key={s}>{s}</th>
              ))}
              <th>开票</th>
              <th>资料</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((c) => (
              <tr
                key={c.id}
                onClick={() => choose(c)}
                className={draft?.id === c.id ? "selected" : ""}
              >
                <td>
                  <b>{c.orderNo}</b>
                  <small>{c.title || "未填写产品"}</small>
                  {overdue(c) && <em>计划逾期</em>}
                </td>
                <td>
                  {c.supplier || "—"}
                  <small>{c.owner || "未分配"}</small>
                </td>
                {c.steps.slice(1).map((s, i) => (
                  <td key={i}>
                    <span
                      className={
                        "pb-badge " +
                        (s.status === "已完成"
                          ? "done"
                          : s.status === "异常"
                            ? "error"
                            : "")
                      }
                    >
                      {s.status}
                    </span>
                    <small>{s.date || s.plan || "未定日期"}</small>
                  </td>
                ))}
                <td>{c.invoice}</td>
                <td>{c.docs.length}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {!shown.length && (
          <div className="pb-empty">
            暂无链路。输入采购单号新建，或从现有采购单建立。
          </div>
        )}
      </div>
      {draft && (
        <section className="pb-detail">
          <div className="pb-detail-head">
            <div>
              <h2>{draft.orderNo || "新建采购链路"}</h2>
              <small>
                {dirty ? "有未保存的修改 · 离开页面前请保存" : "已保存"} · 完成{" "}
                {
                  draft.steps.filter((s) =>
                    ["已完成", "不适用"].includes(s.status),
                  ).length
                }{" "}
                / {stages.length} 环节
              </small>
            </div>
            <div>
              <button
                disabled={busy}
                onClick={() => void save()}
                className="primary"
              >
                {busy ? "处理中…" : "保存链路"}
              </button>
              <button
                disabled={busy}
                onClick={() => {
                  if (!dirty || confirm("放弃未保存修改并关闭？")) {
                    setDraft(null);
                    setDirty(false);
                  }
                }}
              >
                关闭
              </button>
              {draft.revision > 0 && (
                <button
                  disabled={busy}
                  onClick={async () => {
                    if (
                      !confirm(
                        "删除这条统筹链路及其资料副本？原采购单、合同和原文件不会删除。",
                      )
                    )
                      return;
                    setBusy(true);
                    try {
                      await invoke("procurement_board_command", {
                        command: "delete",
                        payload: draft,
                      });
                      setDraft(null);
                      setDirty(false);
                      await load();
                    } catch (e) {
                      setMessage(String(e));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  删除链路
                </button>
              )}
            </div>
          </div>
          <fieldset disabled={busy}>
            <div className="pb-fields">
              {(
                [
                  ["orderNo", "采购单号 *"],
                  ["title", "产品 / 项目"],
                  ["supplier", "供应商"],
                  ["owner", "负责人"],
                ] as const
              ).map(([k, label]) => (
                <label key={k}>
                  {label}
                  <input
                    value={draft[k]}
                    onChange={(e) => edit({ [k]: e.target.value })}
                  />
                </label>
              ))}
              <label>
                开票状态
                <select
                  value={draft.invoice}
                  onChange={(e) => edit({ invoice: e.target.value })}
                >
                  {["未开票", "部分开票", "已开票", "无需开票"].map((v) => (
                    <option key={v}>{v}</option>
                  ))}
                </select>
              </label>
              <label>
                开票日期
                <input
                  type="date"
                  value={draft.invoiceDate}
                  onChange={(e) => edit({ invoiceDate: e.target.value })}
                />
              </label>
            </div>
            <div className="pb-stages">
              {draft.steps.map((s, i) => (
                <article key={i}>
                  <h3>
                    <span>{i + 1}</span>
                    {stages[i]}
                  </h3>
                  <select
                    aria-label={stages[i] + "状态"}
                    value={s.status}
                    onChange={(e) =>
                      edit({
                        steps: draft.steps.map((x, j) =>
                          j === i ? { ...x, status: e.target.value } : x,
                        ),
                      })
                    }
                  >
                    {[
                      "待处理",
                      "进行中",
                      "部分完成",
                      "已完成",
                      "异常",
                      "不适用",
                    ].map((v) => (
                      <option key={v}>{v}</option>
                    ))}
                  </select>
                  {(
                    [
                      ["plan", "计划日期"],
                      ["date", "实际日期"],
                      ["note", "备注 / 运单 / 仓库 / 预约号"],
                    ] as const
                  ).map(([k, label]) => (
                    <label key={k}>
                      {label}
                      <input
                        type={k === "note" ? "text" : "date"}
                        value={s[k]}
                        onChange={(e) =>
                          edit({
                            steps: draft.steps.map((x, j) =>
                              j === i ? { ...x, [k]: e.target.value } : x,
                            ),
                          })
                        }
                      />
                    </label>
                  ))}
                  <div className="pb-docs">
                    {draft.docs
                      .filter((d) => d.stage === i)
                      .map((d) => (
                        <div className="pb-doc" key={d.id}>
                          <b>
                            {d.kind} · {d.name}
                          </b>
                          {d.cells ? (
                            <EditableTable
                              cells={d.cells}
                              onChange={(cells) =>
                                edit({
                                  docs: draft.docs.map((x) =>
                                    x.id === d.id ? { ...x, cells } : x,
                                  ),
                                })
                              }
                            />
                          ) : d.data.startsWith("data:") ? (
                            <a href={d.data} download={d.name}>
                              下载副本
                            </a>
                          ) : (
                            <a href={d.data} target="_blank" rel="noreferrer">
                              打开链接
                            </a>
                          )}
                          <select
                            aria-label={"移动资料 " + d.name}
                            value={d.stage}
                            onChange={(e) =>
                              edit({
                                docs: draft.docs.map((x) =>
                                  x.id === d.id
                                    ? { ...x, stage: Number(e.target.value) }
                                    : x,
                                ),
                              })
                            }
                          >
                            {stages.map((v, n) => (
                              <option key={v} value={n}>
                                移动至 · {v}
                              </option>
                            ))}
                          </select>
                          <button
                            onClick={() => {
                              if (confirm("从链路移除该资料？原文件不受影响。"))
                                edit({
                                  docs: draft.docs.filter((x) => x.id !== d.id),
                                });
                            }}
                          >
                            移除资料
                          </button>
                        </div>
                      ))}
                  </div>
                </article>
              ))}
            </div>
            <div className="pb-add">
              <h3>添加表格 / 附件 / 信息链接</h3>
              <p>
                上传文件保存独立副本；支持 Excel、PDF、Word 和图片。单文件 ≤ 10
                MB；删除或移动资料不会改动原文件。
              </p>
              <div className="pb-fields">
                <label>
                  所属环节
                  <select
                    value={stage}
                    onChange={(e) => setStage(Number(e.target.value))}
                  >
                    {stages.map((s, i) => (
                      <option key={s} value={i}>
                        {s}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  资料类型
                  <select
                    value={kind}
                    onChange={(e) => setKind(e.target.value)}
                  >
                    {[
                      "表格",
                      "采购单",
                      "采购合同",
                      "补货装箱单",
                      "补货装箱表格",
                      "约仓信息",
                      "约仓表格",
                      "付款凭证",
                      "发票",
                      "其他",
                    ].map((v) => (
                      <option key={v}>{v}</option>
                    ))}
                  </select>
                </label>
                <label>
                  上传文件
                  <input
                    type="file"
                    multiple
                    accept=".xlsx,.xls,.csv,.pdf,.doc,.docx,.png,.jpg,.jpeg,.txt"
                    onChange={(e) => {
                      void addFiles(e.target.files);
                      e.target.value = "";
                    }}
                  />
                </label>
                <label>
                  链接名称
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                  />
                </label>
                <label>
                  表格 / 信息链接
                  <input
                    placeholder="https://…"
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                  />
                </label>
                <button onClick={addLink}>添加链接</button>
                <button
                  onClick={() =>
                    edit({
                      docs: [
                        ...draft.docs,
                        {
                          id: crypto.randomUUID(),
                          name: name.trim() || "新建可编辑表格",
                          stage,
                          kind: "在线表格",
                          data: "",
                          cells: [
                            ["字段1", "字段2", "字段3"],
                            ["", "", ""],
                          ],
                        },
                      ],
                    })
                  }
                >
                  ＋ 新建可编辑表格
                </button>
              </div>
            </div>
          </fieldset>
        </section>
      )}
    </div>
  );
}

function EditableTable({
  cells,
  onChange,
}: {
  cells: string[][];
  onChange: (v: string[][]) => void;
}) {
  return (
    <div className="pb-edit-table">
      <div>
        <button
          onClick={() => onChange([...cells, Array(cells[0].length).fill("")])}
        >
          ＋ 行
        </button>
        <button onClick={() => onChange(cells.map((row) => [...row, ""]))}>
          ＋ 列
        </button>
      </div>
      <div style={{ overflowX: "auto" }}>
        <table>
          <thead>
            <tr>
              {cells[0].map((_, j) => (
                <th key={j}>
                  列 {j + 1}
                  <button
                    disabled={cells[0].length <= 1}
                    onClick={() => {
                      if (confirm("删除此列及其内容？"))
                        onChange(
                          cells.map((row) => row.filter((_, k) => k !== j)),
                        );
                    }}
                  >
                    删除列
                  </button>
                </th>
              ))}
              <th />
            </tr>
          </thead>
          <tbody>
            {cells.map((row, i) => (
              <tr key={i}>
                {row.map((value, j) => (
                  <td key={j}>
                    <input
                      aria-label={`第${i + 1}行第${j + 1}列`}
                      value={value}
                      onChange={(e) =>
                        onChange(
                          cells.map((r, n) =>
                            n === i
                              ? r.map((v, k) => (k === j ? e.target.value : v))
                              : r,
                          ),
                        )
                      }
                    />
                  </td>
                ))}
                <td>
                  <button
                    disabled={cells.length <= 1}
                    onClick={() => {
                      if (confirm("删除此行及其内容？"))
                        onChange(cells.filter((_, n) => n !== i));
                    }}
                  >
                    删除行
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <small>第一行可填写字段名称；修改后点击“保存链路”。</small>
    </div>
  );
}
