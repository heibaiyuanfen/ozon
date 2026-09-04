import type {DocumentAst,DocumentNode,ReportNodeType} from "./types";

const customTypes=new Set<ReportNodeType>(["kpi","decision","warning","stage","risk","recommendation","score","action"]);
const idFor=(index:number,type:string)=>`${type}-${index}`;
const isSpecial=(line:string,next="")=>/^\s*(#{1,4})\s+/.test(line)||/^\s*```/.test(line)||/^\s*:::[\w-]+/.test(line)||/^\s*>/.test(line)||/^\s*([-*_])\1{2,}\s*$/.test(line)||/^\s*(?:[-+*]|\d+[.)])\s+/.test(line)||(/^\s*\|/.test(line)&&/^\s*\|?\s*:?-+/.test(next));
const metadata=(content:string)=>Object.fromEntries(content.split(/\r?\n/).map(x=>x.match(/^\s*([^:：]+)\s*[:：]\s*(.+)$/)).filter((x):x is RegExpMatchArray=>!!x).map(x=>[x[1].trim(),x[2].trim()]));

export function parseMarkdownReport(source:string):DocumentAst{
  const lines=source.replace(/\r\n/g,"\n").split("\n"),nodes:DocumentNode[]=[];let i=0,title="可视化运营报告";
  const add=(node:Omit<DocumentNode,"id">)=>nodes.push({id:idFor(nodes.length,node.type),...node});
  while(i<lines.length){const line=lines[i],trim=line.trim();if(!trim){i++;continue;}
    const heading=line.match(/^\s*(#{1,4})\s+(.+)$/);if(heading){const level=heading[1].length;if(level===1&&title==="可视化运营报告")title=heading[2].trim();add({type:"heading",level,content:heading[2].trim()});i++;continue;}
    if(/^\s*```/.test(line)){const language=trim.slice(3).trim().toLowerCase();i++;const body:string[]=[];while(i<lines.length&&!/^\s*```/.test(lines[i]))body.push(lines[i++]);if(i<lines.length)i++;add({type:language==="mermaid"?"mermaid":"code",content:body.join("\n"),metadata:{language}});continue;}
    const custom=trim.match(/^:::([\w-]+)/);if(custom){const kind=custom[1].toLowerCase() as ReportNodeType,i0=i;i++;const body:string[]=[];while(i<lines.length&&lines[i].trim()!==":::")body.push(lines[i++]);if(i<lines.length)i++;add({type:customTypes.has(kind)?kind:"paragraph",content:body.join("\n"),metadata:metadata(body.join("\n"))});if(i===i0)i++;continue;}
    if(/^\s*([-*_])\1{2,}\s*$/.test(line)){add({type:"divider"});i++;continue;}
    if(/^\s*>/.test(line)){const body:string[]=[];while(i<lines.length&&/^\s*>/.test(lines[i]))body.push(lines[i++].replace(/^\s*>\s?/,""));add({type:"quote",content:body.join("\n")});continue;}
    if(/^\s*\|/.test(line)&&/^\s*\|?\s*:?-+/.test(lines[i+1]||"")){const table:string[][]=[];while(i<lines.length&&/^\s*\|/.test(lines[i])){if(table.length!==1)table.push(lines[i].trim().replace(/^\||\|$/g,"").split("|").map(x=>x.trim()));i++;}add({type:"table",rows:table});continue;}
    if(/^\s*(?:[-+*]|\d+[.)])\s+/.test(line)){const items:string[]=[];while(i<lines.length&&/^\s*(?:[-+*]|\d+[.)])\s+/.test(lines[i]))items.push(lines[i++].replace(/^\s*(?:[-+*]|\d+[.)])\s+/,""));add({type:"list",items});continue;}
    const body=[trim];i++;while(i<lines.length&&lines[i].trim()&&!isSpecial(lines[i],lines[i+1]))body.push(lines[i++].trim());add({type:"paragraph",content:body.join("\n")});
  }
  return{title,nodes,source};
}
