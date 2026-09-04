export type ReportNodeType="heading"|"paragraph"|"table"|"mermaid"|"list"|"quote"|"kpi"|"decision"|"warning"|"stage"|"risk"|"recommendation"|"score"|"action"|"divider"|"code";
export interface DocumentNode{id:string;type:ReportNodeType;level?:number;content?:string;items?:string[];rows?:string[][];metadata?:Record<string,string>;}
export interface DocumentAst{title:string;nodes:DocumentNode[];source:string;}
export type ReportThemeName="ozon"|"professional"|"consulting"|"dark"|"minimal";
export type ReportMode="continuous"|"page";
export type ReportOrientation="portrait"|"landscape";
