const OZON_CLUSTER_ZH: Record<string, string> = {
  "Алматы": "阿拉木图",
  "Астана": "阿斯塔纳",
  "Беларусь": "白俄罗斯",
  "Воронеж": "沃罗涅日",
  "Дальний Восток": "远东地区",
  "Екатеринбург": "叶卡捷琳堡",
  "Казань": "喀山",
  "Калининград": "加里宁格勒",
  "Краснодар": "克拉斯诺达尔",
  "Красноярск": "克拉斯诺亚尔斯克",
  "Махачкала": "马哈奇卡拉",
  "Москва, МО и Дальние регионы": "莫斯科、莫斯科州及偏远地区",
  "Невинномысск": "涅温诺梅斯克",
  "Новосибирск": "新西伯利亚",
  "Омск": "鄂木斯克",
  "Оренбург": "奥伦堡",
  "Пермь": "彼尔姆",
  "Ростов": "罗斯托夫",
  "Самара": "萨马拉",
  "Санкт-Петербург и СЗО": "圣彼得堡及西北联邦管区",
  "Саратов": "萨拉托夫",
  "Тверь": "特维尔",
  "Тюмень": "秋明",
  "Уфа": "乌法",
  "Ярославль": "雅罗斯拉夫尔",
};

/** API 原值用于提交；这里只转换用户看到的标签。 */
export function ozonClusterZh(name: string): string {
  const normalized = name.trim();
  if (!normalized) return "未命名集群";
  return OZON_CLUSTER_ZH[normalized] ?? normalized;
}

/** Resolve only known cluster IDs; unknown identifiers remain visible for diagnosis. */
export function ozonClusterIdsLabel(ids: string, names: Record<string, string>): string {
  return ids.replace(/\b\d+\b/g, (id) => names[id]?.trim() ? `${ozonClusterZh(names[id])}（${id}）` : `未知集群（${id}）`);
}
export function ozonDraftErrorLabel(text: string, names: Record<string, string>): string {
  return text.replace(/；集群：([^；]+)/g, (_, ids: string) => `；集群：${ozonClusterIdsLabel(ids, names)}`);
}
