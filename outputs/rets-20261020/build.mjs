import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {Workbook,SpreadsheetFile} from '@oai/artifact-tool';

const out=fileURLToPath(new URL('.',import.meta.url));
const wb=Workbook.create();
const s=wb.worksheets.add('RETS俄罗斯测算');
s.showGridLines=false;
s.getRange('B1:J37').format.font={name:'Arial',size:11,color:'#17243A'};
s.getRange('B1:J37').format.rowHeight=24;
s.getRange('B1').values=[['RETS 俄罗斯运费测算']];
s.getRange('B1').format.font={name:'Arial',size:16,bold:true};
s.getRange('B2:F2').values=[['档位（与图二行序一致）','货值（RUB）','实重（kg）','票费（CNY）','重量费（元/kg）']];
s.getRange('B3:F8').values=[
 ['1 Extra Small Economy','1～1500','≤0.5',3.37,28.1],
 ['2 Small Economy','>1500～7000','≤2',17.97,28.1],
 ['3 Premium Small Economy','>7000～250000','≤5',24.71,28.1],
 ['4 Budget Economy','1～1500','0.5～30',25.83,19.1],
 ['5 Big Standard','>1500～7000','2～30',40.44,28.1],
 ['6 Premium Big Economy','>7000～250000','5～30',69.64,25.8],
];
s.getRange('E3:F8').setNumberFormat('0.00');
s.getRange('I2').values=[['汇率：1 CNY = RUB']]; s.getRange('J2').values=[[12]];
s.getRange('I3:I5').values=[['1500 RUB 对应 CNY'],['7000 RUB 对应 CNY'],['250000 RUB 对应 CNY']];
s.getRange('J3:J5').formulas=[['=IF(AND(ISNUMBER($J$2),$J$2>0),1500/$J$2,"汇率无效")'],['=IF(AND(ISNUMBER($J$2),$J$2>0),7000/$J$2,"汇率无效")'],['=IF(AND(ISNUMBER($J$2),$J$2>0),250000/$J$2,"汇率无效")']];
s.getRange('J2:J5').setNumberFormat('0.0000'); s.getRange('J2').format.fill='#FFF2CC';
s.getRange('I7').values=[['浅黄色为可修改输入']];
s.getRange('B10').values=[['来源：俄通收RETS新品价格测算表2026.07.24执行，Sheet1 俄罗斯/莫斯科区段']];
s.getRange('B11').values=[['费率与原测算公式已核对：G/M 第6、10、17、13、19、21行。Big 为标准线，其余采用经济线。']];
s.getRange('B12').values=[['实际 kg 不进位；重叠端点先取尺寸合规的小件渠道。边界样例保留售价小数用于校验。']];
s.getRange('B10:J12').format.font={name:'Arial',size:10,color:'#65738A'};
s.getRange('B14:J14').values=[['示例 / 可修改输入','实重（kg）','售价（CNY）','长（cm）','宽（cm）','高（cm）','货值（RUB）','档位序号','运费（CNY）']];

function conditions(p,w,sum,max) {
 return [
 `AND(${p}>=1,${p}<=1500,${w}>0,${w}<=0.5,${sum}<=90,${max}<=60)`,
 `AND(${p}>1500,${p}<=7000,${w}>0,${w}<=2,${sum}<=150,${max}<=60)`,
 `AND(${p}>7000,${p}<=250000,${w}>0,${w}<=5,${sum}<=250,${max}<=150)`,
 `AND(${p}>=1,${p}<=1500,${w}>=0.5,${w}<=30,${sum}<=150,${max}<=60)`,
 `AND(${p}>1500,${p}<=7000,${w}>=2,${w}<=30,${sum}<=250,${max}<=150)`,
 `AND(${p}>7000,${p}<=250000,${w}>=5,${w}<=30,${sum}<=310,${max}<=150)`
 ];
}
const samples=[
 ['摄像包示例',.9,130,10,20,30,43.26],['0.3kg',.3,100,10,10,10,11.80],['0.301kg',.301,100,10,10,10,11.83],
 ['401g',.401,100,10,10,10,14.64],['500g 含端点',.5,100,10,10,10,17.42],['500.01g 跨真实门槛',.50001,100,10,10,10,35.38],
 ['1500 RUB 含端点',.4,125,10,10,10,14.61],['1500.01 RUB',.4,1500.01/12,10,10,10,29.21],
 ['7000 RUB 含端点',1,7000/12,10,10,10,46.07],['7000.01 RUB',1,7000.01/12,10,10,10,52.81],
 ['2kg 含端点',2,200,10,10,10,74.17],['2.00001kg',2.00001,200,10,10,10,96.64],
 ['5kg 含端点',5,8000/12,10,10,10,165.21],['5.00001kg',5.00001,8000/12,10,10,10,198.64],
 ['30kg 含端点',30,100,10,10,10,598.83],['30kg 超限',30.00001,100,10,10,10,'不符合运输标准'],
 ['缺少尺寸',.4,100,null,10,10,'请填写长宽高'],['尺寸超限',.4,100,61,1,1,'不符合运输标准'],
 ['Budget 0.53kg',.53,100,10,20,30,35.95],['500g 大尺寸取Budget',.5,100,50,30,20,35.38],
 ['2kg 大尺寸取Big',2,200,80,20,20,96.64],['250000 RUB 含端点',1,250000/12,10,10,10,52.81],
 ['250000.01 RUB 超限',1,250000.01/12,10,10,10,'不符合运输标准'],
];
const formulas=[];
for(let i=0;i<samples.length;i++){
 const r=i+15, sample=samples[i];
 s.getRange(`B${r}:G${r}`).values=[sample.slice(0,6)];
 s.getRange(`H${r}`).formulas=[[`=IF(D${r}="","",IF(AND(ISNUMBER($J$2),$J$2>0),ROUND(D${r}*$J$2,10),"汇率无效"))`]];
 const cond=conditions(`H${r}`,`ROUND(C${r},12)`,`ROUND(SUM(E${r}:G${r}),10)`,`ROUND(MAX(E${r}:G${r}),10)`);
 const ifs=`_xlfn.IFS(${cond.map((c,n)=>`${c},${n+1}`).join(',')},TRUE,"不符合运输标准")`;
 const f=`=IF(OR(C${r}="",D${r}=""),"",IF(OR(NOT(ISNUMBER($J$2)),$J$2<=0),"汇率无效",IF(OR(NOT(ISNUMBER(C${r})),NOT(ISNUMBER(D${r})),C${r}<=0),"价格/重量无效",IF(COUNT(E${r}:G${r})<>3,"请填写长宽高",IF(MIN(E${r}:G${r})<=0,"尺寸无效",${ifs})))))`;
 s.getRange(`I${r}`).formulas=[[f]];
 s.getRange(`J${r}`).formulas=[[`=IF(ISNUMBER(I${r}),ROUND(INDEX($E$3:$E$8,I${r})+ROUND(C${r},12)*INDEX($F$3:$F$8,I${r}),2),I${r})`]];
 formulas.push(f);
}
const last=14+samples.length;
s.getRange(`C15:G${last}`).format.fill='#FFF2CC';
s.getRange(`C15:D${last}`).setNumberFormat('0.000000');
s.getRange(`E15:G${last}`).setNumberFormat('0.0');
s.getRange(`H15:H${last}`).setNumberFormat('0.0000');
s.getRange(`I15:I${last}`).setNumberFormat('0');
s.getRange(`J15:J${last}`).setNumberFormat('0.00');
for(const range of ['B2:F2','B14:J14']) s.getRange(range).format={fill:'#1C3656',font:{name:'Arial',size:11,bold:true,color:'#FFFFFF'},horizontalAlignment:'center',verticalAlignment:'center',wrapText:true,rowHeight:38};
s.getRange('B:B').format.columnWidth=33; s.getRange('C:D').format.columnWidth=23; s.getRange('E:G').format.columnWidth=16;
s.getRange('H:H').format.columnWidth=22; s.getRange('I:I').format.columnWidth=32; s.getRange('J:J').format.columnWidth=25;
s.freezePanes.freezeRows(14);
wb.recalculate();
const values=s.getRange(`J15:J${last}`).values.flat();
for(let i=0;i<samples.length;i++){
 const want=samples[i][6], got=values[i];
 if(typeof want==='number') assert.ok(typeof got==='number'&&Math.abs(got-want)<1e-7,`${samples[i][0]}: ${got} != ${want}`);
 else assert.equal(got,want,samples[i][0]);
}
// Recalculation proof: a fixed CNY price crosses the fixed RUB boundary as FX changes.
s.getRange('J2').values=[[14]]; wb.recalculate();
assert.equal(s.getRange('J21').values[0][0],29.21);
assert.ok(Math.abs(s.getRange('J3').values[0][0]-1500/14)<1e-8);
s.getRange('J2').values=[[0]]; wb.recalculate();
assert.equal(s.getRange('J15').values[0][0],'汇率无效');
assert.deepEqual(s.getRange('J3:J5').values.flat(),['汇率无效','汇率无效','汇率无效']);
s.getRange('J2').values=[[12]]; wb.recalculate();
console.log((await wb.inspect({kind:'table',range:`'RETS俄罗斯测算'!B14:J20`,include:'values,formulas',tableMaxRows:7,tableMaxCols:9,maxChars:2400})).ndjson);
console.log((await wb.inspect({kind:'match',searchTerm:'#REF!|#DIV/0!|#VALUE!|#NAME\\?|#N/A|#NUM!|#NULL!|#SPILL!|#CALC!',options:{useRegex:true,maxResults:50},maxChars:1200,summary:'formula error scan'})).ndjson);
await fs.writeFile(new URL('preview.png',import.meta.url),new Uint8Array(await (await wb.render({sheetName:s.name,range:'B1:J21',scale:1.3,format:'png'})).arrayBuffer()));
await fs.writeFile(new URL('preview-detail.png',import.meta.url),new Uint8Array(await (await wb.render({sheetName:s.name,range:`B22:J${last}`,scale:1.3,format:'png'})).arrayBuffer()));
const file=await SpreadsheetFile.exportXlsx(wb); await file.save(fileURLToPath(new URL('RETS俄罗斯运费测算.xlsx',import.meta.url)));

const cond=conditions('price','wt','size','edge');
const tier=`IFS(${cond.map((c,n)=>`${c},${n+1}`).join(',')},TRUE,0)`;
const letBody=`LET(price,ROUND(F15*$N$2,10),wt,ROUND(E15,12),size,ROUND(SUM(Q15:S15),10),edge,ROUND(MAX(Q15:S15),10),tier,${tier},IF(tier=0,"不符合运输标准",ROUND(INDEX($E$3:$E$8,tier)+wt*INDEX($F$3:$F$8,tier),2)))`;
const standalone=`=IF(OR(F15="",E15=""),"",IF(OR(NOT(ISNUMBER($N$2)),$N$2<=0),"汇率无效",IF(OR(NOT(ISNUMBER(F15)),NOT(ISNUMBER(E15)),E15<=0),"价格/重量无效",IF(COUNT(Q15:S15)<>3,"请填写长宽高",IF(MIN(Q15:S15)<=0,"尺寸无效",${letBody})))))`;
const probe=Workbook.create(); const ps=probe.worksheets.add('Formula');
ps.getRange('E3:F8').values=s.getRange('E3:F8').values;
ps.getRange('N2').values=[[12]];
const compiled=standalone.replace(/\b(price|wt|size|edge|tier)\b/g,'_xlpm.$1').replace(/\bLET\(/g,'_xlfn.LET(').replace(/\bIFS\(/g,'_xlfn.IFS(');
ps.getRange('K15').formulas=[[compiled]];
for(const item of samples) {
 ps.getRange('E15:F15').values=[[item[1],item[2]]]; ps.getRange('Q15:S15').values=[[item[3],item[4],item[5]]];
 probe.recalculate(); const got=ps.getRange('K15').values[0][0]; const want=item[6];
 if(typeof want==='number') assert.ok(typeof got==='number'&&Math.abs(got-want)<1e-7,`standalone ${item[0]}: ${got} != ${want}`);
 else assert.equal(got,want,`standalone ${item[0]}`);
}
await fs.writeFile(new URL('图二_K15公式.txt',import.meta.url),standalone+'\n','utf8');
await fs.writeFile(new URL('图二公式设置.md',import.meta.url),`# 图二 RETS 运费公式设置\n\n采用俄罗斯/莫斯科六档，费用本身是人民币。\n\n1. M2 写“汇率：1 CNY = RUB”，N2 填有效正数，如 12。\n2. E3:F8 依次填写：3.37 / 28.1；17.97 / 28.1；24.71 / 28.1；25.83 / 19.1；40.44 / 28.1；69.64 / 25.8。不要混用其他线路的公式。\n3. 图二 E15 为实重 kg，F15 为人民币售价，Q15/R15/S15 为长宽高 cm；若实际列不同，请替换尺寸引用。\n4. 将“图二_K15公式.txt”整行粘贴到 K15，然后向下填充。已有 LET/IFS 的表格环境可用该公式。\n5. 卢布档位 1500 / 7000 / 250000；人民币分界 = 卢布 ÷ N2。可将 1500/$N$2、7000/$N$2、250000/$N$2 分别作为显示公式。\n\n0.5/2/5/30kg 含端点，重叠端点先选尺寸合规的小件渠道。金额最后保留两位；实重不向上进位。缺少尺寸、超尺寸、超重或货值超限会提示，不输出虚假 0 运费。真实跨档的跳价保留。此表未评价带电/禁运品资质。\n\n随附测算表使用更易核查的中间步骤：先换算货值 RUB，再选择合规档位，最后按票费+实重×重量费计算运费；黄色单元格可改。\n`,'utf8');
console.log(JSON.stringify({output:out,cases:samples.length,fxChangeVerified:true}));
