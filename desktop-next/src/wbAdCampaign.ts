export type WbAdCampaign = { id: number; name: string | null; payment: string | null; status: number | null; products: number[] };
const statuses: Record<number, string> = { 4: "待启动", 7: "已结束", 9: "投放中", 11: "已暂停" };
export const canQueryCampaign = (campaign: WbAdCampaign) => [7, 9, 11].includes(campaign.status ?? 0);
export const campaignLabel = (campaign: WbAdCampaign) =>
  [campaign.name || "未命名活动", campaign.id, campaign.payment?.toUpperCase(),
    statuses[campaign.status ?? 0] ?? "统计不可用"].filter(Boolean).join(" · ");

