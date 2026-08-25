import hashlib
import os
import shutil
import sys
import tempfile
from decimal import Decimal
from pathlib import Path


def prepare_packaged_tk_runtime() -> None:
    """Use a short persistent Tcl/Tk path to avoid Tcl 8.6 long-path failures."""
    if not getattr(sys, "frozen", False):
        return

    bundled_root = Path(getattr(sys, "_MEIPASS", "")).resolve()
    tcl_source = bundled_root / "_tcl_data"
    tk_source = bundled_root / "_tk_data"
    init_source = tcl_source / "init.tcl"
    tk_script_source = tk_source / "tk.tcl"
    if not init_source.is_file() or not tk_script_source.is_file():
        return

    fingerprint = hashlib.sha256(
        init_source.read_bytes() + tk_script_source.read_bytes()
    ).hexdigest()[:12]
    local_app_data = str(os.environ.get("LOCALAPPDATA") or "").strip()
    preferred_root = (
        Path(local_app_data) if local_app_data else Path.home() / "AppData" / "Local"
    ) / "OR"
    candidate_roots = [preferred_root, Path(tempfile.gettempdir()) / "OR"]
    last_error: OSError | None = None
    for candidate_root in dict.fromkeys(candidate_roots):
        tcl_target = candidate_root / "t"
        tk_target = candidate_root / "k"
        complete_marker = candidate_root / ".v"
        try:
            installed_version = (
                complete_marker.read_text(encoding="ascii").strip()
                if complete_marker.is_file() else ""
            )
            if installed_version != fingerprint:
                candidate_root.mkdir(parents=True, exist_ok=True)
                shutil.copytree(tcl_source, tcl_target, dirs_exist_ok=True)
                shutil.copytree(tk_source, tk_target, dirs_exist_ok=True)
                complete_marker.write_text(fingerprint, encoding="ascii")
        except OSError as error:
            last_error = error
            continue
        os.environ["TCL_LIBRARY"] = str(tcl_target)
        os.environ["TK_LIBRARY"] = str(tk_target)
        return
    raise RuntimeError(f"无法准备 Tcl/Tk 界面运行库：{last_error}")


prepare_packaged_tk_runtime()

import tkinter as tk  # noqa: E402

from app import APP_DIR, RfbsListingApp, launch  # noqa: E402


def packaging_smoke_test() -> int:
    """Exercise bundled dynamic dependencies and the persistent data directory."""
    import alibabacloud_oss_v2  # noqa: F401
    import openpyxl
    import playwright.sync_api  # noqa: F401
    from product_ledger import SHEET_TITLE, upsert_product_ledger

    APP_DIR.mkdir(parents=True, exist_ok=True)
    probe = APP_DIR / ".packaging-smoke-test.tmp"
    probe.write_text("ok", encoding="utf-8")
    probe.unlink()
    ledger_probe = APP_DIR / ".packaging-ledger-smoke-test.xlsx"
    try:
        upsert_product_ledger(ledger_probe, {"offer_id": "PACKAGING-SMOKE-TEST"})
        workbook = openpyxl.load_workbook(ledger_probe)
        if SHEET_TITLE not in workbook.sheetnames:
            raise RuntimeError("打包后的 Excel 产品台账模块未能创建工作表")
        workbook.close()
    finally:
        ledger_probe.unlink(missing_ok=True)
    root = tk.Tk()
    root.withdraw()
    app = RfbsListingApp(root)
    manual_pricing = app._job_price_breakdown({
        "purchase_cost": "15", "label_fee": "2", "target_roi": "60",
        "sales_commission_percent": "18", "weight": "600", "fbp_pricing": "0",
    })
    fbp_manual_pricing = app._job_price_breakdown({
        "purchase_cost": "15", "label_fee": "2", "target_roi": "60",
        "sales_commission_percent": "18", "weight": "600", "fbp_pricing": "1",
    })
    if manual_pricing.sales_commission_rate != Decimal("0.18"):
        raise RuntimeError("打包后的 Ozon 人工佣金未参与标准核价")
    if fbp_manual_pricing.sales_commission_rate != Decimal("0.17"):
        raise RuntimeError("打包后的 Ozon FBP 未按人工佣金减 1 个百分点")
    app.job_form_vars["fbp_pricing"].set("1")
    app._on_fbp_pricing_changed()
    if app.job_form_vars["label_fee"].get() != "0":
        raise RuntimeError("打包后的 FBP 核价开关未能清除贴单费")
    if not app.job_label_fee_entry.instate(["disabled"]):
        raise RuntimeError("打包后的 FBP 核价开关未能锁定贴单费输入框")
    app.job_form_vars["listing_mode"].set("local")
    app._on_listing_mode_changed()
    if not app.work_source_entry.instate(["disabled"]):
        raise RuntimeError("打包后的本地新品模式未能停用 Ozon 来源输入框")
    if not app.work_offer_entry.instate(["readonly"]):
        raise RuntimeError("打包后的本地新品模式未能锁定自动 Offer ID")
    if app.job_local_images_button.instate(["disabled"]):
        raise RuntimeError("打包后的本地新品模式未能启用图片选择按钮")
    if not hasattr(app, "wb_product_tabs") or app.wb_product_tabs.index("end") != 3:
        raise RuntimeError("打包后的 WB 上架界面未完整创建")
    app.wb_job = {}
    app.vars["wb_listing_mode"].set("follow")
    app._wb_on_listing_mode_changed()
    if app.wb_source_entry.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 跟卖模式未能启用来源输入框")
    if not app.wb_image_add_button.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 跟卖模式未能锁定本地图片选择")
    app.vars["wb_listing_mode"].set("local")
    app._wb_on_listing_mode_changed()
    if not app.wb_source_entry.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 本地新品模式未能停用来源输入框")
    if app.wb_image_add_button.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 本地新品模式未能启用图片选择")
    app.vars["wb_fulfillment_mode"].set("cross_border")
    app.vars["wb_purchase_cost"].set("15")
    app.vars["wb_label_fee"].set("2")
    app.vars["wb_target_profit_margin_percent"].set("20")
    app.vars["wb_sales_commission_percent"].set("18")
    app.vars["wb_advertising_percent"].set("15")
    app.vars["wb_cargo_loss_percent"].set("10")
    app.vars["wb_cny_rub_rate"].set("")
    app.vars["wb_first_mile_shipping"].set("50")
    app.vars["wb_warehouse_operation_fee"].set("5")
    app.vars["wb_logistics_coefficient_percent"].set("100")
    app.vars["wb_localization_index"].set("1")
    app._wb_on_fulfillment_mode_changed()
    if app.wb_price_currency_var.get() != "自动标价(CNY)":
        raise RuntimeError("打包后的 WB 跨境核价未使用 CNY")
    if "CNY" not in app.wb_pricing_status_var.get():
        raise RuntimeError("打包后的 WB 跨境核价明细未显示 CNY")
    if not app.wb_cny_rub_entry.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 跨境核价不应依赖 CNY→RUB 汇率")
    app.vars["wb_fulfillment_mode"].set("overseas_warehouse")
    app.vars["wb_cny_rub_rate"].set("12")
    app._wb_on_fulfillment_mode_changed()
    if not app.vars["wb_price"].get().strip() or "利润率" not in app.wb_pricing_status_var.get():
        raise RuntimeError("打包后的 WB 海外仓利润率核价未生效")
    if "WB 尾程" not in app.wb_overseas_fee_var.get():
        raise RuntimeError("打包后的 WB 海外仓头程/尾程明细未显示")
    if not app.wb_price_entry.instate(["readonly"]):
        raise RuntimeError("打包后的 WB 自动售价必须保持只读")
    if app.wb_price_currency_var.get() != "自动标价(RUB)":
        raise RuntimeError("打包后的 WB 海外仓核价未使用 RUB")
    if app.wb_cny_rub_entry.instate(["disabled"]):
        raise RuntimeError("打包后的 WB 海外仓核价必须启用 CNY→RUB 汇率")
    if not hasattr(app, "wb_content_json_text"):
        raise RuntimeError("打包后的 WB 内容 JSON/AI 编辑区未创建")
    app.vars["wb_token"].set("")
    app._refresh_wb_config_state()
    if not app.wb_submit_button.instate(["disabled"]):
        raise RuntimeError("WB API 未配置时提交按钮必须保持禁用")
    root.update_idletasks()
    root.destroy()
    return 0


if __name__ == "__main__":
    if "--packaging-smoke-test" in sys.argv:
        raise SystemExit(packaging_smoke_test())
    launch()
