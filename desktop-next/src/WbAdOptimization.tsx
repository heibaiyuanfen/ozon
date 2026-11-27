import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WbCampaignFinance } from "./WbCampaignFinance";
import { WbAdDailyCosts } from "./WbAdDailyCosts";
import { WbKeywordTemplate } from "./WbKeywordTemplate";

import type { WbAdCampaign } from "./wbAdCampaign";

export function WbAdOptimization({ shopId, defaultFrom, defaultTo, maxDate }: {
  shopId: string; defaultFrom: string; defaultTo: string; maxDate: string;
}) {
  const [campaigns, setCampaigns] = useState<WbAdCampaign[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const rows = await invoke<WbAdCampaign[]>("wb_operations", { action: "ad_campaigns", payload: { shopId } });
      setCampaigns(rows.sort((a, b) => Number(b.status === 9) - Number(a.status === 9) || b.id - a.id));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, [shopId]);
  useEffect(() => { void refresh(); }, [refresh]);

  return <>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>广告活动 · {campaigns.length}</h2>
        <button type="button" disabled={loading} onClick={() => void refresh()}>{loading ? "正在读取…" : "刷新广告活动"}</button>
      </div>
      {error && <p role="alert" className="wbo-error">{error}</p>}
      {!loading && !error && !campaigns.length && <p>当前店铺暂无广告活动。</p>}
    </section>
    <WbCampaignFinance shopId={shopId} campaigns={campaigns} defaultFrom={defaultFrom} defaultTo={defaultTo} maxDate={maxDate} />
    <WbAdDailyCosts shopId={shopId} campaigns={campaigns} defaultFrom={defaultFrom} defaultTo={defaultTo} maxDate={maxDate} />
    <WbKeywordTemplate shopId={shopId} campaigns={campaigns} defaultEnd={defaultTo} />
  </>;
}
