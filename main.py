from pathlib import Path
import sys
from datetime import date, datetime, timedelta


SOURCE_ROOT = Path(__file__).resolve().parent
RUNTIME_ROOT = (
    Path(sys.executable).resolve().parent
    if getattr(sys, "frozen", False)
    else SOURCE_ROOT
)
if str(SOURCE_ROOT) not in sys.path:
    sys.path.insert(0, str(SOURCE_ROOT))

from ozon_app.ui import OzonAnalyticsApp


def run_scheduled_feishu_weekly() -> None:
    from ozon_app.database import Database
    from ozon_app.services import AppService
    from ozon_app.shops import ShopRegistry

    registry = ShopRegistry(RUNTIME_ROOT / "data")
    database = Database(registry.database_path(registry.active()))
    database.close_stale_running_logs()
    if database.get_setting("feishu_weekly_auto_enabled", "1") == "0":
        return
    today = date.today()
    target = today - timedelta(days=today.weekday() + 1)
    if database.get_setting("feishu_weekly_last_sent_end") >= target.isoformat():
        return
    log_id = database.start_log("Feishu Scheduler")
    database.append_log_event(
        log_id, "INFO", "计划任务", "Windows 计划任务开始检查最新完整周",
        {"target_week_end": target.isoformat()},
    )
    try:
        active_shop = registry.active()
        service = AppService(
            database, shop_name=active_shop.name, shop_kind=active_shop.kind,
        )
        credentials = service.load_credentials()
        feishu_ready = all((
            credentials.feishu_app_id, credentials.feishu_app_secret,
            credentials.feishu_app_token, credentials.feishu_weekly_table_id,
            credentials.feishu_chat_id,
        ))
        if not feishu_ready:
            raise RuntimeError("飞书计划任务缺少完整的应用、周报表或群聊配置。")
        week_start = target - timedelta(days=6)
        cache = database.sales_sync_state(week_start.isoformat(), target.isoformat())
        seller_verified = str(cache.get("verified_through") or "") >= target.isoformat()
        if not seller_verified:
            if not credentials.seller_client_id or not credentials.seller_api_key:
                raise RuntimeError("最新完整周尚未缓存，且缺少 Seller API 凭证。")
            service.sync_seller(week_start.isoformat(), target.isoformat())
            refreshed = database.sales_sync_state(
                week_start.isoformat(), target.isoformat()
            )
            if str(refreshed.get("verified_through") or "") < target.isoformat():
                raise RuntimeError("Seller 周报数据尚未核验完整，本次不发送飞书周报。")
        if not credentials.performance_client_id or not credentials.performance_client_secret:
            raise RuntimeError("缺少 Performance API 凭证，无法刷新周报广告数据。")
        service.sync_performance(week_start.isoformat(), target.isoformat())
        database.save_settings({"weekly_last_synced_end": target.isoformat()})
        service.sync_feishu_weekly(target.isoformat(), credentials=credentials)
        database.save_settings({
            "feishu_weekly_last_sent_end": target.isoformat(),
            "feishu_weekly_last_sent_at": datetime.now().isoformat(timespec="seconds"),
        })
        database.append_log_event(
            log_id, "SUCCESS", "结束", "计划任务已完成周报写表和发群",
            {"target_week_end": target.isoformat()},
        )
        database.finish_log(log_id, "success", 1, "飞书周报计划任务完成")
    except Exception as error:
        database.append_log_event(
            log_id, "ERROR", "失败", "飞书周报计划任务失败", {"error": str(error)},
        )
        database.finish_log(log_id, "failed", 0, str(error))
        raise


def main() -> None:
    if "--weekly-feishu-sync" in sys.argv:
        run_scheduled_feishu_weekly()
        return
    app = OzonAnalyticsApp(RUNTIME_ROOT)
    if "--packaging-smoke-test" in sys.argv:
        from tempfile import TemporaryDirectory

        from ozon_app.exports import (
            export_ai_docx, export_ai_pdf, export_replenishment_xlsx,
        )

        app.withdraw()
        timer = getattr(app, "_weekly_timer_id", None)
        if timer:
            app.after_cancel(timer)
        app.update_idletasks()
        app.update()
        app.destroy()
        with TemporaryDirectory(prefix="ozon_package_test_") as folder:
            target = Path(folder)
            report_text = "# 打包测试\n\nWord 与 PDF 导出组件可用。"
            word_path = export_ai_docx(
                target / "test.docx", report_text, "2026-08-01", "2026-08-07", "package-test"
            )
            pdf_path = export_ai_pdf(
                target / "test.pdf", report_text, "2026-08-01", "2026-08-07", "package-test"
            )
            excel_path = export_replenishment_xlsx(
                target / "test.xlsx",
                [{
                    "cluster_name": "测试集群", "macrolocal_cluster_id": "1",
                    "available_stock": 10, "transit_stock": 2, "requested_stock": 0,
                    "daily_sales": 1.5, "planned_qty": 33,
                }],
                {"sku": "100", "offer_id": "TEST", "product_name": "测试商品"},
                30, "直送", {"city": "测试城市", "address": "测试地址"},
            )
            if any(path.stat().st_size == 0 for path in (word_path, pdf_path, excel_path)):
                raise RuntimeError("Packaged report export smoke test produced an empty file.")
        return
    app.mainloop()


if __name__ == "__main__":
    main()
