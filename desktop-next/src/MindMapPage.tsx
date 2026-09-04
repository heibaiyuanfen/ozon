import {useDeferredValue,useEffect,useMemo,useRef,useState} from "react";
import {Download,FileDown,FileText,FolderOpen,ImageDown,Minus,Plus,Printer,RefreshCw,Save} from "lucide-react";
import {exportReportPdf,exportReportPng} from "./report-renderer/export";
import {MarkdownReportRenderer} from "./report-renderer/MarkdownReportRenderer";
import {parseMarkdownReport} from "./report-renderer/parser";
import type {ReportMode,ReportOrientation,ReportThemeName} from "./report-renderer/types";
import "./report-renderer/report.css";

const sample=`# Ozon 新品推流策略

当前阶段以完整自然日数据判断，每次只调整一个变量。

:::kpi
售价: 190 ₽
周广告预算: 10,000 ₽
目标 ACOS: 15%
目标广告单: 50 单/天
:::

## 增长决策流程

\`\`\`mermaid
flowchart TD
  A["190 ₽ + 10k 广告"] --> B{"广告单 ≥ 50 单/天？"}
  B -->|达标| C["增加预算或提高价格"]
  B -->|未达标| D["维持预算并优化"]
  C --> E{"新阶段数据稳定？"}
  E -->|达标| F["继续爬坡"]
  E -->|未达标| G["回退上一级"]
  F --> H["300 ₽ × 200 单/天"]
\`\`\`

:::decision
条件: CVR ≥ 4.5%，CPA ≤ 180 ₽
达标动作: 增加广告预算 20%
未达标动作: 保持预算或回退
:::

:::warning
如果新增广告投入不能带来同比订单增长，则停止放量。
:::

## 执行纪律

- 不使用当天未结束数据
- 至少观察 3 天或累计 100 次点击
- 边际 ROAS 低于保本线立即回退
`;
const download=(name:string,text:string)=>{const url=URL.createObjectURL(new Blob([text],{type:"text/markdown;charset=utf-8"})),a=document.createElement("a");a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};

export function MindMapPage({shopId}:{shopId:string}){
  const storageKey=`ozon-visual-report:${shopId}`,[source,setSource]=useState(()=>localStorage.getItem(storageKey)||sample),[mode,setMode]=useState<ReportMode>("continuous"),[orientation,setOrientation]=useState<ReportOrientation>("portrait"),[theme,setTheme]=useState<ReportThemeName>("ozon"),[zoom,setZoom]=useState(1),[message,setMessage]=useState(""),[dragging,setDragging]=useState(false),[busy,setBusy]=useState(false),fileRef=useRef<HTMLInputElement>(null),reportRef=useRef<HTMLDivElement>(null),deferred=useDeferredValue(source);
  useEffect(()=>{setSource(localStorage.getItem(storageKey)||sample);setMessage("");},[storageKey]);
  const ast=useMemo(()=>parseMarkdownReport(deferred),[deferred]),safeName=ast.title.replace(/[\\/:*?"<>|]/g,"-").slice(0,60)||"Ozon运营报告";
  const loadFile=(file?:File)=>{if(!file)return;if(file.size>5*1024*1024){setMessage("文件超过 5MB，请拆分后再读取");return;}const reader=new FileReader();reader.onload=()=>{setSource(String(reader.result||""));setMessage(`已读取 ${file.name}`);};reader.onerror=()=>setMessage("文件读取失败");reader.readAsText(file,"UTF-8");};
  const run=async(label:string,task:()=>Promise<void>)=>{if(!reportRef.current)return;setBusy(true);setMessage(`${label}处理中…`);try{await task();setMessage(`${label}已完成`);}catch(e){setMessage(`${label}失败：${String(e)}`);}finally{setBusy(false);}};
  return <>
    <header className="page-header"><div><span className="eyebrow">MARKDOWN REPORT ENGINE</span><h1>可视化报告</h1><p>Markdown → Document AST → 业务组件与 Mermaid SVG → Web / A4 / PDF / PNG</p></div></header>
    <section className="mind-map-toolbar card"><div className="report-toolbar"><button onClick={()=>fileRef.current?.click()}><FolderOpen/>上传 MD</button><input ref={fileRef} hidden type="file" accept=".md,.markdown,.mmd,.mermaid,.txt" onChange={e=>loadFile(e.target.files?.[0])}/><button onClick={()=>{setSource(localStorage.getItem(storageKey)||sample);setMessage("已重新加载草稿");}}><RefreshCw/>重新加载</button><button onClick={()=>{localStorage.setItem(storageKey,source);setMessage("已保存当前店铺草稿");}}><Save/>保存</button><select value={mode} onChange={e=>setMode(e.target.value as ReportMode)}><option value="continuous">连续模式</option><option value="page">A4 页面</option></select><select value={orientation} onChange={e=>setOrientation(e.target.value as ReportOrientation)}><option value="portrait">纵向</option><option value="landscape">横向</option></select><select value={theme} onChange={e=>setTheme(e.target.value as ReportThemeName)}><option value="ozon">Ozon Operations</option><option value="professional">Professional Light</option><option value="consulting">Consulting Blue</option><option value="minimal">Minimal</option><option value="dark">Dark Report</option></select><button onClick={()=>setZoom(v=>Math.max(.5,v-.1))}><Minus/>缩小</button><button onClick={()=>setZoom(v=>Math.min(1.6,v+.1))}><Plus/>{Math.round(zoom*100)}%</button><button disabled={busy} onClick={()=>void run("PDF",()=>exportReportPdf(reportRef.current!,safeName,orientation))}><FileDown/>PDF</button><button disabled={busy} onClick={()=>void run("PNG",()=>exportReportPng(reportRef.current!,safeName))}><ImageDown/>PNG</button><button onClick={()=>window.print()}><Printer/>打印</button><button onClick={()=>download(`${safeName}.md`,source)}><Download/>源文件</button></div><span className="report-message">{message||`${ast.nodes.length} 个结构块；Mermaid 保持 SVG`}</span></section>
    <section className={`report-shell ${dragging?"report-drop-active":""}`} onDragOver={e=>{e.preventDefault();setDragging(true);}} onDragLeave={()=>setDragging(false)} onDrop={e=>{e.preventDefault();setDragging(false);loadFile(e.dataTransfer.files[0]);}}>
      <div className="card report-editor"><header><div><FileText/><b>Markdown / Ozon 报告 DSL</b></div><small>支持拖入文件</small></header><textarea value={source} onChange={e=>setSource(e.target.value)} spellCheck={false}/><div className="report-message" style={{padding:"9px 14px"}}>支持 :::kpi、decision、stage、risk、warning、recommendation、score、action</div></div>
      <div className="card report-preview"><header><div><b>专业报告预览</b><small>{mode==="page"?`A4 ${orientation==="portrait"?"纵向":"横向"}`:"连续阅读"}</small></div><span>颜色同时配有“达标 / 观察 / 风险”文字</span></header><div className="report-preview-scroll"><MarkdownReportRenderer ast={ast} theme={theme} mode={mode} orientation={orientation} zoom={zoom} reportRef={reportRef}/></div></div>
    </section>
  </>;
}
