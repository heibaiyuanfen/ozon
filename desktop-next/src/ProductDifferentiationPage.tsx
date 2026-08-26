import { useMemo, useState } from "react";
import { CheckCircle2, Plus, Save, Target, Trash2 } from "lucide-react";
import "./product-differentiation.css";

type Candidate = { asin: string; title: string; price: string; rating: string; features: string; negativeReviews: string };
type Draft = { asin: string; title: string; category: string; price: string; rating: string; features: string; positiveReviews: string; targetCustomer: string; marketNotes: string; targetPrice: string; logistics: string; localization: string; competitors: Candidate[] };
const blankCompetitor = (): Candidate => ({ asin: "", title: "", price: "", rating: "", features: "", negativeReviews: "" });
const initial: Draft = { asin: "", title: "", category: "", price: "", rating: "", features: "", positiveReviews: "", targetCustomer: "", marketNotes: "", targetPrice: "", logistics: "", localization: "", competitors: [blankCompetitor(), blankCompetitor()] };
const lines = (value: string) => value.split(/\n|,|，|;|；/).map((x) => x.trim()).filter(Boolean);
const usd = (value: string) => value && Number.isFinite(Number(value)) ? `$${Number(value).toFixed(2)}` : "—";

export function ProductDifferentiationPage() {
  const [draft, setDraft] = useState<Draft>(() => { try { return JSON.parse(localStorage.getItem("amazon-differentiation-draft") || "null") || initial; } catch { return initial; } });
  const [saved, setSaved] = useState(false);
  const analysis = useMemo(() => {
    const ownFeatures = lines(draft.features), competitors = draft.competitors.filter((x) => x.title || x.asin);
    const negative = competitors.flatMap((x) => lines(x.negativeReviews)), positive = lines(draft.positiveReviews);
    const marketReady = Boolean(draft.targetCustomer && draft.marketNotes && draft.localization);
    const level = marketReady && negative.length && positive.length ? 4 : positive.length && negative.length ? 3 : negative.length ? 2 : draft.title && competitors.length ? 1 : 0;
    const competitorFeatures = new Set(competitors.flatMap((x) => lines(x.features).map((y) => y.toLowerCase())));
    const unique = ownFeatures.filter((x) => !competitorFeatures.has(x.toLowerCase()));
    const painGroups = new Map<string, number>();
    const painKeys = ["质量", "损坏", "尺寸", "气味", "包装", "续航", "连接", "噪音", "材质", "不舒服", "дорог", "качест", "размер", "упаков"];
    negative.forEach((review) => { const key = painKeys.find((x) => review.toLowerCase().includes(x)) || review.slice(0, 18); painGroups.set(key, (painGroups.get(key) || 0) + 1); });
    const pains = [...painGroups.entries()].sort((a, b) => b[1] - a[1]).slice(0, 6);
    const prices = competitors.map((x) => Number(x.price)).filter((x) => x > 0), avgPrice = prices.length ? prices.reduce((a, b) => a + b, 0) / prices.length : null;
    let russianScore = 20;
    if (draft.targetCustomer) russianScore += 20; if (draft.localization) russianScore += 20; if (draft.logistics) russianScore += 15; if (draft.marketNotes) russianScore += 15; if (draft.targetPrice) russianScore += 10;
    return { ownFeatures, competitors, positive, level, unique, pains, avgPrice, russianScore };
  }, [draft]);
  const set = (key: keyof Draft, value: string) => setDraft((old) => ({ ...old, [key]: value }));
  const setCompetitor = (index: number, key: keyof Candidate, value: string) => setDraft((old) => ({ ...old, competitors: old.competitors.map((x, i) => i === index ? { ...x, [key]: value } : x) }));
  const save = () => { localStorage.setItem("amazon-differentiation-draft", JSON.stringify(draft)); setSaved(true); window.setTimeout(() => setSaved(false), 1800); };
  return <div className="differentiation-page">
    <header className="page-header"><div><span className="eyebrow">AMAZON → RUSSIA</span><h1>亚马逊差异化选品</h1><p>从 Amazon 竞品弱点和评论洞察出发，筛选适合俄罗斯市场的差异化机会</p></div><button className="dark-button" onClick={save}>{saved ? <CheckCircle2 size={16}/> : <Save size={16}/>} {saved ? "已保存" : "保存本地草稿"}</button></header>
    <section className="diff-level card"><Target size={21}/><div><b>当前分析深度 L{analysis.level}</b><span>{analysis.level === 0 ? "填写产品和至少一个竞品后开始" : analysis.level === 1 ? "基础竞品矩阵" : analysis.level === 2 ? "已解锁竞品痛点" : analysis.level === 3 ? "已解锁 USP 提取" : "完整策略与俄罗斯适配"}</span></div><div className="level-track">{[1,2,3,4].map((x)=><i className={analysis.level >= x ? "done" : ""} key={x}>L{x}</i>)}</div></section>
    <div className="diff-editor-grid">
      <section className="card diff-form"><div className="card-title">候选产品</div><div className="diff-fields">
        <label>Amazon ASIN<input value={draft.asin} onChange={(e)=>set("asin",e.target.value)} placeholder="例如 B0..."/></label><label>产品名称<input value={draft.title} onChange={(e)=>set("title",e.target.value)} placeholder="候选产品名称"/></label><label>类目<input value={draft.category} onChange={(e)=>set("category",e.target.value)} placeholder="Amazon 类目"/></label><label>售价 USD<input type="number" min="0" value={draft.price} onChange={(e)=>set("price",e.target.value)}/></label><label>评分<input type="number" min="0" max="5" step="0.1" value={draft.rating} onChange={(e)=>set("rating",e.target.value)}/></label>
        <label className="wide">功能/属性（每行一项）<textarea value={draft.features} onChange={(e)=>set("features",e.target.value)} placeholder={'耐低温\n俄语说明书\n加固包装'}/></label><label className="wide">自己的正面评论/卖点证据（每行一条）<textarea value={draft.positiveReviews} onChange={(e)=>set("positiveReviews",e.target.value)} placeholder="没有真实评论时请留空，不会自动虚构"/></label>
      </div></section>
      <section className="card russia-fit"><div className="card-title">俄罗斯市场适配输入 <span className="badge blue">人工判断</span></div><label>目标人群<input value={draft.targetCustomer} onChange={(e)=>set("targetCustomer",e.target.value)} placeholder="地区、年龄、使用场景"/></label><label>市场与季节需求<textarea value={draft.marketNotes} onChange={(e)=>set("marketNotes",e.target.value)} placeholder="寒冷气候、节日、当地使用习惯等"/></label><label>目标零售价（RUB）<input type="number" min="0" value={draft.targetPrice} onChange={(e)=>set("targetPrice",e.target.value)}/></label><label>物流/尺寸限制<textarea value={draft.logistics} onChange={(e)=>set("logistics",e.target.value)} placeholder="重量、易碎、跨境时效、仓储限制"/></label><label>本地化与合规<textarea value={draft.localization} onChange={(e)=>set("localization",e.target.value)} placeholder="俄语包装、EAC/类目认证、插头或尺码适配"/></label><div className="fit-score"><span>资料准备度</span><b>{analysis.russianScore}%</b><i><em style={{width:`${analysis.russianScore}%`}}/></i><small>这是资料完整度，不代表真实市场成功率。</small></div></section>
    </div>
    <section className="card competitor-editor"><div className="card-title">Amazon 竞品输入 <button onClick={()=>setDraft((old)=>({...old,competitors:[...old.competitors,blankCompetitor()]}))}><Plus size={15}/>添加竞品</button></div>{draft.competitors.map((row,index)=><div className="competitor-input" key={index}><div className="competitor-number">#{index+1}</div><input value={row.asin} onChange={(e)=>setCompetitor(index,"asin",e.target.value)} placeholder="ASIN"/><input value={row.title} onChange={(e)=>setCompetitor(index,"title",e.target.value)} placeholder="竞品名称"/><input type="number" min="0" value={row.price} onChange={(e)=>setCompetitor(index,"price",e.target.value)} placeholder="价格 USD"/><input type="number" min="0" max="5" step="0.1" value={row.rating} onChange={(e)=>setCompetitor(index,"rating",e.target.value)} placeholder="评分"/><textarea value={row.features} onChange={(e)=>setCompetitor(index,"features",e.target.value)} placeholder="功能，每行一项"/><textarea value={row.negativeReviews} onChange={(e)=>setCompetitor(index,"negativeReviews",e.target.value)} placeholder="真实负评，每行一条"/><button className="icon-delete" disabled={draft.competitors.length===1} onClick={()=>setDraft((old)=>({...old,competitors:old.competitors.filter((_,i)=>i!==index)}))}><Trash2 size={15}/></button></div>)}</section>
    <div className="diff-results-grid">
      <section className="card diff-result"><div className="card-title">竞品对比矩阵</div><div className="matrix-scroll"><table><thead><tr><th>维度</th><th>候选产品</th>{analysis.competitors.map((x,i)=><th key={i}>{x.title || x.asin}</th>)}</tr></thead><tbody><tr><td>价格</td><td>{usd(draft.price)}</td>{analysis.competitors.map((x,i)=><td key={i}>{usd(x.price)}</td>)}</tr><tr><td>评分</td><td>{draft.rating || "—"}</td>{analysis.competitors.map((x,i)=><td key={i}>{x.rating || "—"}</td>)}</tr><tr><td>功能数</td><td>{analysis.ownFeatures.length}</td>{analysis.competitors.map((x,i)=><td key={i}>{lines(x.features).length}</td>)}</tr></tbody></table></div>{analysis.avgPrice != null && <p>竞品平均价 ${analysis.avgPrice.toFixed(2)}；候选产品相对均价 {Number(draft.price) ? `${((Number(draft.price)/analysis.avgPrice-1)*100).toFixed(1)}%` : "—"}。</p>}</section>
      <section className="card diff-result"><div className="card-title">竞品负评痛点</div>{analysis.pains.length ? <ol>{analysis.pains.map(([x,n])=><li key={x}><b>{x}</b><span>{n} 条提及</span></li>)}</ol> : <div className="empty compact">填写真实负评后解锁 L2</div>}</section>
      <section className="card diff-result"><div className="card-title">可验证 USP</div>{analysis.level >= 3 ? <ul>{analysis.unique.map((x)=><li key={x}>{x}</li>)}{analysis.positive.slice(0,3).map((x)=><li key={x}>{x}</li>)}</ul> : <div className="empty compact">填写自己的正面评论证据后解锁 L3</div>}</section>
      <section className="card diff-result action-plan"><div className="card-title">差异化行动计划</div><div><b>高优先级</b><p>{analysis.pains[0] ? `围绕“${analysis.pains[0][0]}”设计可量化改进，并在样品测试中验证。` : "先收集至少 3 个核心竞品的真实负评。"}</p></div><div><b>中优先级</b><p>{analysis.unique[0] ? `将“${analysis.unique[0]}”做成主图、标题和对比图可理解的卖点。` : "补充候选产品与竞品功能清单，识别真正的功能缺口。"}</p></div><div><b>俄罗斯适配</b><p>{analysis.level === 4 ? "验证俄语本地化、目标卢布价、物流尺寸和类目合规后，再进入打样与小批量测试。" : "补齐目标人群、市场需求和本地化/合规信息，解锁完整策略。"}</p></div></section>
    </div>
  </div>;
}
