from __future__ import annotations

import json
import queue
import re
import shutil
import threading
import time
import uuid
import tkinter as tk
from decimal import Decimal, InvalidOperation, ROUND_CEILING
from pathlib import Path
from tkinter import filedialog, messagebox, ttk

from core import (
    ProductInput, ReferenceProduct, atomic_write_json, attribute_values_by_id,
    build_import_item, calculate_price_breakdown, calculate_roi_price, clean_attribute_payload,
    export_reference, find_reference_fact,
    flatten_categories, format_ozon_hashtags, load_json, normalize_attribute_name,
    parse_attributes, parse_complex_attributes, parse_ozon_hashtags,
    scrape_reference, set_attribute_value,
    unsupported_chinese_submission_fields, validate_ozon_hashtags, validate_required,
)
from services import (
    CopywritingService, ImageDownloadService, ImageGenerationService,
    OssService, OzonApi, OzonDictionaryCache, ProductVisionAnalysisService,
    WatermarkService,
    build_ozon_main_image_prompt, build_ozon_poster_text_lines,
)
from product_ledger import upsert_product_ledger
from runtime_paths import APP_DATA_DIR
from wb_ui import WbUiMixin


APP_DIR = APP_DATA_DIR
CONFIG_PATH = APP_DIR / "config.json"
WORKSPACE_PATH = APP_DIR / "workspace_state.json"
OUTPUT_DIR = APP_DIR / "输出"
DICTIONARY_CACHE_DIR = APP_DIR / "cache" / "ozon-dictionaries"
CATEGORY_CACHE_PATH = APP_DIR / "cache" / "ozon-categories.json"
AUTO_JOBS_PATH = APP_DIR / "auto_jobs.json"
PRODUCT_LEDGER_PATH = APP_DIR / "产品台账.xlsx"
WB_JOB_PATH = APP_DIR / "wb_job.json"
NO_BRAND_DICTIONARY_ID = 126745801
NO_BRAND_DICTIONARY_VALUE = "Нет бренда"
WORKSPACE_FIELD_KEYS = (
    "listing_mode", "source_url", "supplier_1688_url", "title", "tags", "price", "old_price", "currency_code", "net_weight", "weight", "weight_unit",
    "depth", "width", "height", "dimension_unit", "vat", "offer_id", "model_name", "image_prompt",
    "watermark_path", "category_display", "category_id", "type_id", "warehouse_id", "stock",
    "purchase_cost", "label_fee", "target_roi", "sales_commission_percent",
    "advertising_percent", "cargo_loss_percent", "fbp_pricing",
)
JOB_INPUT_KEYS = (
    "listing_mode", "ozon_shop_id", "ozon_shop_name", "source_url", "supplier_1688_url", "offer_id", "model_name", "price", "old_price", "net_weight", "weight",
    "depth", "width", "height", "category_display", "category_id", "type_id",
    "warehouse_id", "stock", "watermark_path", "image_prompt", "currency_code",
    "weight_unit", "dimension_unit", "vat", "purchase_cost", "label_fee", "target_roi",
    "sales_commission_percent", "advertising_percent", "cargo_loss_percent", "fbp_pricing",
)


class AutoJobCancelled(RuntimeError):
    """Cooperative stop requested for one queued automatic listing job."""


class RfbsListingApp(WbUiMixin):
    WB_CONFIG_PATH = CONFIG_PATH
    WB_JOB_PATH = WB_JOB_PATH
    OUTPUT_DIR = OUTPUT_DIR
    PRODUCT_LEDGER_PATH = PRODUCT_LEDGER_PATH

    def __init__(self, root: tk.Tk):
        self.root = root
        self.root.title("Ozon / Wildberries 上品工具")
        self.root.geometry("1180x900")
        self.root.minsize(980, 760)
        self.log_queue: queue.Queue[str] = queue.Queue()
        self.busy = False
        self.reference = ReferenceProduct("", "")
        self.categories: list[dict] = []
        self.category_displays: list[str] = []
        self.category_by_display: dict[str, dict] = {}
        self.category_search_text_by_display: dict[str, str] = {}
        self.attribute_definitions: list[dict] = []
        self.attribute_definition_by_id: dict[int, dict] = {}
        self.attribute_fact_suggestions: dict[int, tuple[str, str]] = {}
        self.attribute_dictionary_results: dict[str, dict] = {}
        self.attribute_dictionary_all_results: list[dict] = []
        self.attribute_dictionary_attribute_id = 0
        self.vision_analysis: dict = {}
        self.dictionary_cache = OzonDictionaryCache(DICTIONARY_CACHE_DIR)
        self.warehouse_by_display: dict[str, dict] = {}
        self.ozon_shops: dict[str, dict] = {}
        self.ozon_shop_id_by_display: dict[str, str] = {}
        self.ozon_shop_display_by_id: dict[str, str] = {}
        self.source_images: list[str] = []
        self.local_images: list[str] = []
        self.uploaded_urls: list[str] = []
        self.primary_reference_index = 0
        self._closing = False
        self._workspace_save_error = ""
        self.auto_pipeline_running = False
        self.auto_progress_value = 0
        self.vars: dict[str, tk.StringVar] = {}
        self.job_form_vars: dict[str, tk.StringVar] = {}
        self.job_form_image_paths: list[str] = []
        self.auto_jobs: dict[str, dict] = {}
        self.auto_job_order: list[str] = []
        self.auto_job_queue: queue.Queue[str] = queue.Queue()
        self.auto_job_lock = threading.RLock()
        self.auto_job_worker_running = False
        self.active_job_id = ""
        self._initialize_wb_state()
        self._build()
        self._load_config()
        self._load_wb_job()
        self._load_category_cache()
        self._restore_workspace()
        self._initialize_job_form_from_workspace()
        self._load_auto_jobs()
        self.root.protocol("WM_DELETE_WINDOW", self._on_close)
        self.root.after(100, self._poll_log)
        self.root.after(15000, self._autosave_workspace)

    def _var(self, key: str, default="") -> tk.StringVar:
        if key in self.vars:
            return self.vars[key]
        value = tk.StringVar(value=default)
        self.vars[key] = value
        return value

    def _job_var(self, key: str, default="") -> tk.StringVar:
        if key in self.job_form_vars:
            return self.job_form_vars[key]
        value = tk.StringVar(value=default)
        self.job_form_vars[key] = value
        return value

    def _job_entry(self, parent, label, key, row, column=0, *, width=34, default=""):
        ttk.Label(parent, text=label).grid(row=row, column=column, sticky="w", padx=5, pady=4)
        entry = ttk.Entry(parent, textvariable=self._job_var(key, default), width=width)
        entry.grid(row=row, column=column + 1, sticky="ew", padx=5, pady=4)
        return entry

    @staticmethod
    def _fbp_pricing_enabled(values: dict) -> bool:
        return str(values.get("fbp_pricing") or "").strip().casefold() in {
            "1", "true", "yes", "on",
        }

    @staticmethod
    def _local_listing_enabled(values: dict) -> bool:
        return str(values.get("listing_mode") or "follow").strip().casefold() == "local"

    def _active_listing_is_local(self) -> bool:
        active_job_id = str(getattr(self, "active_job_id", "") or "")
        active_job = getattr(self, "auto_jobs", {}).get(active_job_id, {})
        if active_job:
            return self._local_listing_enabled(active_job.get("inputs") or {})
        variable = getattr(self, "vars", {}).get("listing_mode")
        return self._local_listing_enabled({
            "listing_mode": variable.get() if variable is not None else "follow",
        })

    @staticmethod
    def _job_price_breakdown(values: dict):
        weight_kg = Decimal(str(values.get("weight") or "").strip()) / Decimal("1000")
        fbp_pricing = RfbsListingApp._fbp_pricing_enabled(values)
        return calculate_roi_price(
            values.get("purchase_cost", ""),
            "0" if fbp_pricing else values.get("label_fee", ""),
            values.get("target_roi", ""), weight_kg,
            sales_commission_percent=values.get("sales_commission_percent", ""),
            sales_commission_discount_percent="1" if fbp_pricing else "0",
            advertising_percent=values.get("advertising_percent") or "15",
            cargo_loss_percent=values.get("cargo_loss_percent") or "10",
        )

    @staticmethod
    def _pricing_amount(value: Decimal) -> str:
        return format(value.quantize(Decimal("0.01")), "f")

    def _update_job_price(self, *_args):
        keys = (
            "purchase_cost", "label_fee", "target_roi", "advertising_percent",
            "cargo_loss_percent", "sales_commission_percent", "weight",
        )
        values = {
            key: self.job_form_vars[key].get().strip()
            for key in keys if key in self.job_form_vars
        }
        values["fbp_pricing"] = (
            self.job_form_vars["fbp_pricing"].get().strip()
            if "fbp_pricing" in self.job_form_vars else "0"
        )
        if any(key not in values or values[key] == "" for key in keys):
            if "price" in self.job_form_vars:
                self.job_form_vars["price"].set("")
            if getattr(self, "price_breakdown_var", None) is not None:
                self.price_breakdown_var.set("费用预估：待计算")
            return
        try:
            breakdown = self._job_price_breakdown(values)
        except (InvalidOperation, ValueError) as error:
            self.job_form_vars["price"].set("")
            self.price_breakdown_var.set("费用预估：" + str(error))
            return
        self.job_form_vars["price"].set(format(breakdown.price, "f"))
        pricing_mode = "FBP" if self._fbp_pricing_enabled(values) else "RFBS"
        self.price_breakdown_var.set(
            "{mode} 费用预估：运费 {shipping} | 销售佣金 {sales_rate}%：{sales} | 物流佣金 {logistics} | "
            "广告 {advertising_rate}%：{advertising} | 货损 {loss_rate}%：{loss} | "
            "利润 {profit} | 实际 ROI {roi}%".format(
                mode=pricing_mode,
                shipping=self._pricing_amount(breakdown.shipping),
                sales_rate=self._pricing_amount(breakdown.sales_commission_rate * Decimal("100")),
                sales=self._pricing_amount(breakdown.sales_commission),
                logistics=self._pricing_amount(breakdown.logistics_commission),
                advertising_rate=values["advertising_percent"],
                advertising=self._pricing_amount(breakdown.advertising),
                loss_rate=values["cargo_loss_percent"],
                loss=self._pricing_amount(breakdown.cargo_loss),
                profit=self._pricing_amount(breakdown.profit),
                roi=self._pricing_amount(breakdown.roi_percent),
            )
        )

    def _on_fbp_pricing_changed(self):
        if getattr(self, "_updating_fbp_pricing", False):
            return
        self._updating_fbp_pricing = True
        try:
            enabled = self._fbp_pricing_enabled({
                "fbp_pricing": self._job_var("fbp_pricing", "0").get(),
            })
            label_variable = self._job_var("label_fee", "2")
            if enabled:
                current = label_variable.get().strip()
                if current and current != "0":
                    self._normal_label_fee_before_fbp = current
                label_variable.set("0")
                self.job_label_fee_entry.state(["disabled"])
            else:
                self.job_label_fee_entry.state(["!disabled"])
                if label_variable.get().strip() in {"", "0"}:
                    label_variable.set(
                        str(getattr(self, "_normal_label_fee_before_fbp", "2") or "2")
                    )
        finally:
            self._updating_fbp_pricing = False
        self._update_job_price()

    def _build(self):
        self.notebook = ttk.Notebook(self.root)
        self.notebook.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        self.workbench_tab = ttk.Frame(self.notebook, padding=12)
        self.settings_tab = ttk.Frame(self.notebook, padding=12)
        self.product_tab = ttk.Frame(self.notebook, padding=12)
        self.attributes_tab = ttk.Frame(self.notebook, padding=12)
        self.execute_tab = ttk.Frame(self.notebook, padding=12)
        self.wb_tab = ttk.Frame(self.notebook, padding=12)
        self.notebook.add(self.workbench_tab, text="1. 自动工作台")
        self.notebook.add(self.settings_tab, text="2. 服务配置")
        self.notebook.add(self.product_tab, text="3. 商品与图片")
        self.notebook.add(self.attributes_tab, text="4. 类目与属性")
        self.notebook.add(self.execute_tab, text="5. 草稿与上架")
        self.notebook.add(self.wb_tab, text="6. WB 上架")
        self._build_workbench()
        self._build_settings()
        self._build_product()
        self._build_attributes()
        self._build_execute()
        self._build_wb()

    def _build_workbench(self):
        tab = self.workbench_tab
        tab.columnconfigure(1, weight=1)
        tab.columnconfigure(3, weight=1)
        ttk.Label(
            tab, text="填写一次，自动完成解析、文案、图片、OSS、属性、Ozon 导入和 RFBS 库存",
            font=("Microsoft YaHei UI", 13, "bold"),
        ).grid(row=0, column=0, columnspan=2, sticky="w", pady=(0, 12))
        self._var("listing_mode", "follow")
        mode_frame = ttk.Frame(tab)
        mode_frame.grid(row=0, column=2, columnspan=2, sticky="e", pady=(0, 12))
        ttk.Radiobutton(
            mode_frame, text="跟卖模式", variable=self._job_var("listing_mode", "follow"),
            value="follow", command=self._on_listing_mode_changed,
        ).pack(side=tk.LEFT, padx=4)
        ttk.Radiobutton(
            mode_frame, text="本地新品模式", variable=self._job_var("listing_mode", "follow"),
            value="local", command=self._on_listing_mode_changed,
        ).pack(side=tk.LEFT, padx=4)
        self.work_source_label = ttk.Label(tab, text="Ozon 链接 / 商品 ID")
        self.work_source_label.grid(row=1, column=0, sticky="w", padx=5, pady=4)
        self.work_source_entry = ttk.Entry(
            tab, textvariable=self._job_var("source_url"), width=70,
        )
        self.work_source_entry.grid(row=1, column=1, sticky="ew", padx=5, pady=4)
        ttk.Label(tab, text="上品店铺").grid(row=1, column=2, sticky="w", padx=5, pady=4)
        self.work_shop_combo = ttk.Combobox(
            tab, textvariable=self._job_var("ozon_shop_display"), state="readonly", width=34,
        )
        self.work_shop_combo.grid(row=1, column=3, sticky="ew", padx=5, pady=4)
        self.work_shop_combo.bind("<<ComboboxSelected>>", lambda _event: self._work_shop_selected())
        self.work_offer_entry = self._job_entry(
            tab, "货号（Offer ID）", "offer_id", 2,
            default=f"AUTO-{time.strftime('%Y%m%d')}-{uuid.uuid4().hex[:6].upper()}",
        )
        self._job_entry(tab, "型号名称（同名合并多变体）", "model_name", 2, 2)
        self._job_entry(tab, "1688 采购链接（可选）", "supplier_1688_url", 3, width=70)
        ttk.Label(tab, text="本地成品图片").grid(row=3, column=2, sticky="w", padx=5, pady=4)
        local_images = ttk.Frame(tab)
        local_images.grid(row=3, column=3, sticky="ew", padx=5, pady=4)
        self.job_local_images_button = ttk.Button(
            local_images, text="选择图片", command=self._select_job_local_images,
        )
        self.job_local_images_button.pack(side=tk.LEFT)
        self.job_local_images_clear_button = ttk.Button(
            local_images, text="清空", command=self._clear_job_local_images,
        )
        self.job_local_images_clear_button.pack(side=tk.LEFT, padx=(5, 8))
        self.job_local_images_status_var = tk.StringVar(value="跟卖模式无需选择")
        ttk.Label(
            local_images, textvariable=self.job_local_images_status_var, foreground="#245a8d",
        ).pack(side=tk.LEFT, fill=tk.X, expand=True)
        pricing_defaults = (
            ("purchase_cost", ""), ("label_fee", "2"), ("target_roi", "60"),
            ("sales_commission_percent", ""),
            ("advertising_percent", "15"), ("cargo_loss_percent", "10"),
        )
        for key, default in pricing_defaults:
            self._var(key, default)
            self._job_var(key, default)
        self._var("fbp_pricing", "0")
        self._job_var("fbp_pricing", "0")
        self._normal_label_fee_before_fbp = "2"
        pricing = ttk.LabelFrame(tab, text="ROI 自动定价", padding=8)
        pricing.grid(row=4, column=0, columnspan=4, sticky="ew", pady=(5, 6))
        for column in (1, 3, 5, 7):
            pricing.columnconfigure(column, weight=1)
        for column, label, key in (
            (0, "采购成本", "purchase_cost"), (2, "贴单费", "label_fee"),
            (4, "目标 ROI（%）", "target_roi"), (6, "自动售价", "price"),
        ):
            ttk.Label(pricing, text=label).grid(row=0, column=column, sticky="w", padx=4, pady=3)
            entry = ttk.Entry(pricing, textvariable=self._job_var(key), width=13)
            entry.grid(row=0, column=column + 1, sticky="ew", padx=4, pady=3)
            if key == "label_fee":
                self.job_label_fee_entry = entry
            if key == "price":
                entry.configure(state="readonly")
        ttk.Label(pricing, text="标准佣金（%）").grid(row=1, column=0, sticky="w", padx=4, pady=3)
        ttk.Entry(
            pricing, textvariable=self._job_var("sales_commission_percent"), width=13,
        ).grid(row=1, column=1, sticky="ew", padx=4, pady=3)
        ttk.Checkbutton(
            pricing, text="FBP 核价", variable=self._job_var("fbp_pricing", "0"),
            onvalue="1", offvalue="0", command=self._on_fbp_pricing_changed,
        ).grid(row=1, column=2, columnspan=2, sticky="w", padx=4, pady=3)
        ttk.Label(pricing, text="广告费率（%）").grid(row=1, column=4, sticky="w", padx=4, pady=3)
        ttk.Entry(
            pricing, textvariable=self._job_var("advertising_percent", "15"), width=13,
        ).grid(row=1, column=5, sticky="ew", padx=4, pady=3)
        ttk.Label(pricing, text="货损率（%）").grid(row=1, column=6, sticky="w", padx=4, pady=3)
        ttk.Entry(
            pricing, textvariable=self._job_var("cargo_loss_percent", "10"), width=13,
        ).grid(row=1, column=7, sticky="ew", padx=4, pady=3)
        ttk.Label(pricing, text="折扣前价格").grid(row=2, column=0, sticky="w", padx=4, pady=3)
        ttk.Entry(pricing, textvariable=self._job_var("old_price", "0"), width=13).grid(
            row=2, column=1, sticky="ew", padx=4, pady=3,
        )
        ttk.Label(
            pricing,
            text="佣金由人工填写；FBP 勾选后按填写值减 1 个百分点，不再读取 Ozon 佣金",
            foreground="#8a5a00",
        ).grid(row=2, column=2, columnspan=6, sticky="w", padx=4, pady=3)
        self.price_breakdown_var = tk.StringVar(value="费用预估：待计算")
        ttk.Label(pricing, textvariable=self.price_breakdown_var, foreground="#245a8d").grid(
            row=3, column=0, columnspan=8, sticky="w", padx=4, pady=3,
        )
        self._job_entry(tab, "商品重量/净重（克）", "net_weight", 5, default="900")
        self._job_entry(tab, "毛重（含包装，克）", "weight", 5, 2, default="1000")
        self._job_entry(tab, "包装长度/深度（毫米）", "depth", 6, default="100")
        self._job_entry(tab, "包装宽度（毫米）", "width", 6, 2, default="100")
        self._job_entry(tab, "包装高度（毫米）", "height", 7, default="100")
        self._job_entry(tab, "RFBS 仓库 ID", "warehouse_id", 7, 2)
        self._job_var("weight_unit", "g")
        self._job_var("dimension_unit", "mm")
        self._job_var("currency_code", "RUB")
        self._job_var("vat", "0")
        for key in (
            "purchase_cost", "label_fee", "target_roi", "advertising_percent",
            "cargo_loss_percent", "sales_commission_percent", "weight",
        ):
            self.job_form_vars[key].trace_add("write", self._update_job_price)

        ttk.Label(tab, text="Ozon 类目（必须人工指定）").grid(row=8, column=0, sticky="w", padx=5, pady=6)
        self.work_category_combo = ttk.Combobox(
            tab, textvariable=self._job_var("category_display"), state="normal", width=85,
        )
        self.work_category_combo.grid(row=8, column=1, columnspan=2, sticky="ew", padx=5, pady=6)
        self.work_category_combo.bind("<<ComboboxSelected>>", lambda _event: self._job_category_selected())
        self.work_category_combo.bind("<Return>", lambda _event: self._job_category_selected())
        self.work_category_combo.bind("<KeyRelease>", self._filter_category_choices)
        ttk.Button(
            tab, text="加载/刷新类目", command=lambda: self._run(self._load_categories_with_retry),
        ).grid(row=8, column=3, sticky="w", padx=5)
        self.work_category_id_label = ttk.Label(tab, text="尚未选择实时类目", foreground="#666")
        self.work_category_id_label.grid(row=9, column=1, columnspan=3, sticky="w", padx=5)

        action = ttk.LabelFrame(tab, text="自动流程", padding=10)
        action.grid(row=10, column=0, columnspan=4, sticky="ew", pady=(14, 8))
        action.columnconfigure(1, weight=1)
        self.auto_start_button = ttk.Button(
            action, text="加入队列并自动运行", command=self._enqueue_job_from_form,
        )
        self.auto_start_button.grid(row=0, column=0, rowspan=2, padx=(0, 12), sticky="ns")
        self.auto_progress = ttk.Progressbar(action, mode="determinate", maximum=9)
        self.auto_progress.grid(row=0, column=1, sticky="ew")
        self.auto_status_var = tk.StringVar(value="等待录入；真实上架会在启动前集中确认一次")
        ttk.Label(action, textvariable=self.auto_status_var, foreground="#245a8d").grid(row=1, column=1, sticky="w", pady=(5, 0))
        ttk.Label(
            tab,
            text="自动重试：页面读取/下载、文案 AI、图片生成、OSS、类目属性读取和库存写入。\n"
                 "商品创建请求不会盲目重发；若响应丢失，请按 Offer ID 到 Ozon 后台核对，避免重复创建。",
            foreground="#8a5a00", justify="left",
        ).grid(row=11, column=0, columnspan=4, sticky="w", padx=5, pady=8)

        jobs_frame = ttk.LabelFrame(tab, text="产品任务队列（可在上一件运行时继续填写并加入下一件）", padding=8)
        jobs_frame.grid(row=12, column=0, columnspan=4, sticky="nsew", pady=(6, 0))
        jobs_frame.columnconfigure(0, weight=1)
        jobs_frame.rowconfigure(0, weight=1)
        self.job_tree = ttk.Treeview(
            jobs_frame, columns=("shop", "mode", "offer", "model", "product", "status", "stage", "error"),
            show="headings", height=7, selectmode="browse",
        )
        for column, title, width in (
            ("shop", "店铺", 105), ("mode", "模式", 75), ("offer", "货号", 115), ("model", "型号名称", 130), ("product", "来源/图片", 145),
            ("status", "状态", 80), ("stage", "进度", 65), ("error", "错误/说明", 340),
        ):
            self.job_tree.heading(column, text=title)
            self.job_tree.column(column, width=width, stretch=column in {"product", "error"})
        self.job_tree.grid(row=0, column=0, sticky="nsew")
        job_scroll = ttk.Scrollbar(jobs_frame, orient="vertical", command=self.job_tree.yview)
        job_scroll.grid(row=0, column=1, sticky="ns")
        self.job_tree.configure(yscrollcommand=job_scroll.set)
        controls = ttk.Frame(jobs_frame)
        controls.grid(row=1, column=0, columnspan=2, sticky="w", pady=(7, 0))
        ttk.Button(controls, text="修复选中任务", command=self._open_job_repair).pack(side=tk.LEFT, padx=(0, 6))
        ttk.Button(controls, text="继续/重试选中任务", command=self._retry_selected_job).pack(side=tk.LEFT, padx=6)
        ttk.Button(controls, text="停止选中任务", command=self._stop_selected_job).pack(side=tk.LEFT, padx=6)
        ttk.Button(controls, text="清除已结束任务", command=self._clear_finished_jobs).pack(side=tk.LEFT, padx=6)
        self._on_listing_mode_changed()

    def _set_job_local_image_paths(self, paths):
        unique_paths = []
        seen = set()
        for raw_path in paths:
            path = str(Path(raw_path))
            key = path.casefold()
            if key in seen:
                continue
            seen.add(key)
            unique_paths.append(path)
            if len(unique_paths) >= 15:
                break
        self.job_form_image_paths = unique_paths
        status = getattr(self, "job_local_images_status_var", None)
        if status is not None:
            if unique_paths:
                status.set(f"已选择 {len(unique_paths)} 张；第 1 张为主图")
            elif self._local_listing_enabled({"listing_mode": self._job_var("listing_mode", "follow").get()}):
                status.set("尚未选择图片")
            else:
                status.set("跟卖模式无需选择")

    def _select_job_local_images(self):
        paths = filedialog.askopenfilenames(
            title="选择本地新品图片（第 1 张为主图）",
            filetypes=[("图片", "*.jpg *.jpeg *.png *.webp")],
        )
        if paths:
            self._set_job_local_image_paths(paths)

    def _clear_job_local_images(self):
        self._set_job_local_image_paths([])

    def _on_listing_mode_changed(self):
        mode = self._job_var("listing_mode", "follow").get().strip() or "follow"
        local_mode = self._local_listing_enabled({"listing_mode": mode})
        if not str(getattr(self, "active_job_id", "") or ""):
            self._var("listing_mode", "follow").set(mode)
        if getattr(self, "work_source_entry", None) is not None:
            self.work_source_entry.state(["disabled"] if local_mode else ["!disabled"])
            self.work_source_label.configure(
                text="Ozon 来源（本地新品无需填写）" if local_mode else "Ozon 链接 / 商品 ID",
            )
        if getattr(self, "work_offer_entry", None) is not None:
            self.work_offer_entry.state(["readonly"] if local_mode else ["!readonly"])
        for button_name in ("job_local_images_button", "job_local_images_clear_button"):
            button = getattr(self, button_name, None)
            if button is not None:
                button.state(["!disabled"] if local_mode else ["disabled"])
        self._set_job_local_image_paths(getattr(self, "job_form_image_paths", []))
        if getattr(self, "product_source_entry", None) is not None:
            self.product_source_entry.state(["disabled"] if local_mode else ["!disabled"])
        for button_name in ("product_parse_button", "product_download_button", "product_generate_images_button"):
            button = getattr(self, button_name, None)
            if button is not None:
                button.state(["disabled"] if local_mode else ["!disabled"])

    def _entry(self, parent, label, key, row, column=0, *, width=34, show=None, default=""):
        ttk.Label(parent, text=label).grid(row=row, column=column, sticky="w", padx=5, pady=4)
        entry = ttk.Entry(parent, textvariable=self._var(key, default), width=width, show=show)
        entry.grid(row=row, column=column + 1, sticky="ew", padx=5, pady=4)
        return entry

    def _build_settings(self):
        tab = self.settings_tab
        tab.columnconfigure(1, weight=1)
        tab.columnconfigure(3, weight=1)
        ttk.Label(tab, text="Ozon Seller API 多店铺", font=("Microsoft YaHei UI", 12, "bold")).grid(row=0, column=0, columnspan=4, sticky="w", pady=(0, 8))
        ttk.Label(tab, text="已保存店铺").grid(row=1, column=0, sticky="w", padx=5, pady=4)
        self.settings_shop_combo = ttk.Combobox(
            tab, textvariable=self._var("ozon_shop_display"), state="readonly", width=55,
        )
        self.settings_shop_combo.grid(row=1, column=1, sticky="ew", padx=5, pady=4)
        self.settings_shop_combo.bind("<<ComboboxSelected>>", lambda _event: self._settings_shop_selected())
        shop_buttons = ttk.Frame(tab)
        shop_buttons.grid(row=1, column=2, columnspan=2, sticky="w", padx=5)
        ttk.Button(shop_buttons, text="新建店铺", command=self._new_ozon_shop).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(shop_buttons, text="保存/更新店铺", command=self._save_current_ozon_shop).pack(side=tk.LEFT, padx=5)
        ttk.Button(shop_buttons, text="删除店铺", command=self._delete_current_ozon_shop).pack(side=tk.LEFT, padx=5)
        self._entry(tab, "店铺名称", "ozon_shop_name", 2)
        self._entry(tab, "Client-Id", "ozon_client_id", 2, 2)
        self._entry(tab, "Api-Key", "ozon_api_key", 3, show="*")
        self._entry(tab, "Ozon HTTP代理（可选）", "ozon_proxy_url", 3, 2, width=42)
        ttk.Button(tab, text="测试当前店铺连接", command=lambda: self._run(self._test_ozon_connection)).grid(row=4, column=0, sticky="w", padx=5)
        ttk.Label(tab, text="密钥仅保存在本机 config.json；任务队列只记录店铺 ID 和名称", foreground="#666").grid(row=4, column=1, columnspan=3, sticky="w", padx=5)
        ttk.Separator(tab).grid(row=5, column=0, columnspan=4, sticky="ew", pady=12)
        ttk.Label(tab, text="独立生图服务（OpenAI-compatible Images Edits）", font=("Microsoft YaHei UI", 12, "bold")).grid(row=6, column=0, columnspan=4, sticky="w")
        self._entry(tab, "接口地址", "image_api_url", 7, width=50)
        self._entry(tab, "API Key", "image_api_key", 7, 2, show="*")
        self._entry(tab, "模型", "image_model", 8, default="gpt-image-1")
        ttk.Separator(tab).grid(row=9, column=0, columnspan=4, sticky="ew", pady=10)
        ttk.Label(tab, text="产品视觉分析 AI（OpenAI-compatible Chat Completions，须支持图片）", font=("Microsoft YaHei UI", 12, "bold")).grid(row=10, column=0, columnspan=4, sticky="w")
        self._entry(tab, "接口地址", "analysis_api_url", 11, width=50)
        self._entry(tab, "API Key", "analysis_api_key", 11, 2, show="*")
        self._entry(tab, "模型", "analysis_model", 12)
        ttk.Label(tab, text="三项留空时复用文案 AI；复用模型必须支持多图视觉输入", foreground="#666").grid(row=12, column=2, columnspan=2, sticky="w", padx=5)
        ttk.Separator(tab).grid(row=13, column=0, columnspan=4, sticky="ew", pady=10)
        ttk.Label(tab, text="独立文案服务（OpenAI-compatible Chat Completions）", font=("Microsoft YaHei UI", 12, "bold")).grid(row=14, column=0, columnspan=4, sticky="w")
        self._entry(tab, "接口地址", "text_api_url", 15, width=50)
        self._entry(tab, "API Key", "text_api_key", 15, 2, show="*")
        self._entry(tab, "模型", "text_model", 16)
        ttk.Separator(tab).grid(row=17, column=0, columnspan=4, sticky="ew", pady=10)
        ttk.Label(tab, text="阿里云 OSS", font=("Microsoft YaHei UI", 12, "bold")).grid(row=18, column=0, columnspan=4, sticky="w")
        self._entry(tab, "Region", "oss_region", 19, default="cn-hangzhou")
        self._entry(tab, "Endpoint", "oss_endpoint", 19, 2, default="oss-cn-hangzhou.aliyuncs.com")
        self._entry(tab, "Bucket", "oss_bucket", 20)
        self._entry(tab, "公共域名", "oss_public_base_url", 20, 2)
        self._entry(tab, "AccessKey ID", "oss_access_key_id", 21)
        self._entry(tab, "AccessKey Secret", "oss_access_key_secret", 21, 2, show="*")
        self._entry(tab, "对象前缀", "oss_object_prefix", 22, default="rfbs-listing")
        ttk.Button(tab, text="保存全部配置", command=self._save_config).grid(row=23, column=3, sticky="e", padx=5, pady=12)
        ttk.Label(tab, text="配置保存在本机 config.json；请勿上传或分享密钥文件。", foreground="#8a5a00").grid(row=24, column=0, columnspan=4, sticky="w")

    def _build_product(self):
        tab = self.product_tab
        tab.columnconfigure(1, weight=1)
        tab.columnconfigure(3, weight=1)
        self.product_source_entry = self._entry(tab, "Ozon 链接 / Артикул", "source_url", 0, width=70)
        self.product_parse_button = ttk.Button(
            tab, text="读取指定变体", command=lambda: self._run(self._parse_url),
        )
        self.product_parse_button.grid(row=0, column=2, padx=5)
        self._entry(tab, "俄文标题", "title", 1, width=70)
        ttk.Button(tab, text="生成俄文标题/描述/标签", command=lambda: self._run(self._generate_copywriting)).grid(row=1, column=2, padx=5)
        ttk.Label(tab, text="商品描述").grid(row=2, column=0, sticky="nw", padx=5, pady=4)
        self.description_text = tk.Text(tab, height=4, wrap="word")
        self.description_text.grid(row=2, column=1, columnspan=3, sticky="nsew", padx=5, pady=4)
        self._entry(tab, "搜索标签", "tags", 3, width=70)
        ttk.Label(
            tab, text="25–30 个；多词用下划线；单个含 # 最多 30 字符", foreground="#245a8d",
        ).grid(row=3, column=2, columnspan=2, sticky="w", padx=5)
        fields = [
            ("1688 采购链接（可选）", "supplier_1688_url", ""),
            ("型号名称（同名合并多变体）", "model_name", ""),
            ("售价", "price", "999"), ("折扣前价格", "old_price", "0"),
            ("币种", "currency_code", "RUB"), ("VAT", "vat", "0"),
            ("商品重量/净重（克）", "net_weight", "900"), ("毛重（含包装）", "weight", "1000"),
            ("重量单位", "weight_unit", "g"), ("长度/深度", "depth", "100"),
            ("宽度", "width", "100"),
            ("高度", "height", "100"), ("尺寸单位", "dimension_unit", "mm"),
            ("货号（Offer ID）", "offer_id", f"AUTO-{time.strftime('%Y%m%d')}-{uuid.uuid4().hex[:6].upper()}"),
        ]
        row = 4
        for index in range(0, len(fields), 2):
            left = fields[index]
            self._entry(tab, left[0], left[1], row, default=left[2])
            if index + 1 < len(fields):
                right = fields[index + 1]
                self._entry(tab, right[0], right[1], row, 2, default=right[2])
            row += 1
        image_frame = ttk.LabelFrame(tab, text="本地商品图片", padding=8)
        image_frame.grid(row=row, column=0, columnspan=4, sticky="nsew", pady=10)
        image_frame.columnconfigure(0, weight=1)
        self.image_list = tk.Listbox(image_frame, height=7, selectmode=tk.EXTENDED)
        self.image_list.grid(row=0, column=0, rowspan=8, sticky="nsew", padx=(0, 8))
        self.image_list.bind("<<ListboxSelect>>", self._image_selection_changed)
        ttk.Button(image_frame, text="选择参考图", command=self._select_images).grid(row=0, column=1, sticky="ew", pady=2)
        ttk.Button(image_frame, text="移除选中", command=self._remove_images).grid(row=1, column=1, sticky="ew", pady=2)
        self.product_download_button = ttk.Button(
            image_frame, text="重新下载原图", command=lambda: self._run(self._download_images),
        )
        self.product_download_button.grid(row=2, column=1, sticky="ew", pady=2)
        self.product_generate_images_button = ttk.Button(
            image_frame, text="生成新主图并处理附图", command=lambda: self._run(self._generate_images),
        )
        self.product_generate_images_button.grid(row=3, column=1, sticky="ew", pady=2)
        ttk.Button(image_frame, text="再次添加水印", command=lambda: self._run(self._watermark_images)).grid(row=4, column=1, sticky="ew", pady=2)
        ttk.Button(image_frame, text="上传 OSS", command=lambda: self._run(self._upload_images)).grid(row=5, column=1, sticky="ew", pady=2)
        ttk.Button(image_frame, text="重新分析全部原图", command=lambda: self._run(self._reanalyze_images)).grid(row=6, column=1, sticky="ew", pady=2)
        ttk.Label(image_frame, text="分析 AI 看全部原图；生图模型只用单选主体图（不选＝第1张）", foreground="#245a8d", wraplength=190).grid(row=7, column=1, sticky="w", pady=(5, 0))
        self._entry(tab, "生图提示词", "image_prompt", row + 1, width=80, default="Create a polished category-appropriate marketplace scene with designed lighting, natural depth, strong product separation and a controlled palette.")
        self._entry(tab, "水印文件", "watermark_path", row + 2, width=55)
        ttk.Button(tab, text="选择", command=self._select_watermark).grid(row=row + 2, column=2, sticky="w")

    def _build_attributes(self):
        tab = self.attributes_tab
        tab.columnconfigure(1, weight=1)
        tab.rowconfigure(3, weight=3)
        tab.rowconfigure(6, weight=2)
        ttk.Button(tab, text="读取 Ozon 中文实时类目", command=lambda: self._run(self._load_categories_with_retry)).grid(row=0, column=0, padx=5, pady=5, sticky="w")
        self.category_combo = ttk.Combobox(
            tab, textvariable=self._var("category_display"), state="normal", width=90,
        )
        self.category_combo.grid(row=0, column=1, sticky="ew", padx=5)
        self.category_combo.bind("<<ComboboxSelected>>", lambda _event: self._category_selected())
        self.category_combo.bind("<Return>", lambda _event: self._category_selected())
        self.category_combo.bind("<KeyRelease>", self._filter_category_choices)
        ttk.Button(tab, text="读取属性并按页面事实预填", command=lambda: self._run(self._load_attributes)).grid(row=0, column=2, padx=5)
        self._entry(tab, "类目 ID", "category_id", 1)
        self._entry(tab, "类型 ID", "type_id", 1, 2)

        ttk.Button(
            tab, text="AI 填写当前类目属性（不改类目）",
            command=lambda: self._run(self._ai_fill_selected_category_attributes),
        ).grid(row=2, column=0, sticky="w", padx=5, pady=(7, 3))
        self.required_label = ttk.Label(tab, text="类目必须人工指定；AI 只填写当前类目的属性，不会改变类目")
        self.required_label.grid(row=2, column=1, columnspan=2, sticky="w", padx=5, pady=(7, 3))

        table_frame = ttk.Frame(tab)
        table_frame.grid(row=3, column=0, columnspan=3, sticky="nsew", padx=5)
        table_frame.columnconfigure(0, weight=1)
        table_frame.rowconfigure(0, weight=1)
        self.attribute_tree = ttk.Treeview(
            table_frame, columns=("required", "kind", "value", "source"),
            show="tree headings", selectmode="browse", height=12,
        )
        self.attribute_tree.heading("#0", text="Ozon 实时属性（中文）")
        self.attribute_tree.heading("required", text="必填")
        self.attribute_tree.heading("kind", text="填写方式")
        self.attribute_tree.heading("value", text="当前值")
        self.attribute_tree.heading("source", text="依据")
        self.attribute_tree.column("#0", width=270, stretch=True)
        self.attribute_tree.column("required", width=52, anchor="center", stretch=False)
        self.attribute_tree.column("kind", width=90, anchor="center", stretch=False)
        self.attribute_tree.column("value", width=310, stretch=True)
        self.attribute_tree.column("source", width=135, stretch=False)
        self.attribute_tree.grid(row=0, column=0, sticky="nsew")
        attribute_scroll = ttk.Scrollbar(table_frame, orient="vertical", command=self.attribute_tree.yview)
        attribute_scroll.grid(row=0, column=1, sticky="ns")
        self.attribute_tree.configure(yscrollcommand=attribute_scroll.set)
        self.attribute_tree.bind("<<TreeviewSelect>>", lambda _event: self._attribute_tree_selected())

        editor = ttk.LabelFrame(tab, text="选中属性编辑（字典会加载全部分页值并缓存到本机）", padding=6)
        editor.grid(row=4, column=0, columnspan=3, sticky="ew", padx=5, pady=6)
        editor.columnconfigure(1, weight=1)
        editor.columnconfigure(4, weight=1)
        self.selected_attribute_label = ttk.Label(editor, text="请先在上表选择属性", wraplength=980)
        self.selected_attribute_label.grid(row=0, column=0, columnspan=5, sticky="w", pady=(0, 4))
        ttk.Button(editor, text="清除当前值", command=self._clear_selected_attribute).grid(row=0, column=5, padx=3, pady=(0, 4))
        ttk.Label(editor, text="文本值").grid(row=1, column=0, sticky="w")
        self.attribute_value_var = tk.StringVar()
        ttk.Entry(editor, textvariable=self.attribute_value_var).grid(row=1, column=1, sticky="ew", padx=5)
        ttk.Button(editor, text="保存文本值", command=self._save_selected_attribute_text).grid(row=1, column=2, padx=3)
        ttk.Label(editor, text="本地筛选").grid(row=1, column=3, sticky="w", padx=(12, 0))
        self.dictionary_query_var = tk.StringVar()
        dictionary_filter_entry = ttk.Entry(editor, textvariable=self.dictionary_query_var)
        dictionary_filter_entry.grid(row=1, column=4, sticky="ew", padx=5)
        dictionary_filter_entry.bind("<KeyRelease>", self._filter_dictionary_choices)
        ttk.Button(editor, text="加载全部选项", command=self._begin_dictionary_load).grid(row=1, column=5, padx=3)
        ttk.Label(editor, text="返回值").grid(row=2, column=0, sticky="w", pady=(5, 0))
        self.dictionary_combo = ttk.Combobox(editor, state="readonly", width=72)
        self.dictionary_combo.grid(row=2, column=1, columnspan=4, sticky="ew", padx=5, pady=(5, 0))
        ttk.Button(editor, text="采用选中值", command=self._apply_dictionary_choice).grid(row=2, column=5, padx=3, pady=(5, 0))
        self.dictionary_status_label = ttk.Label(editor, text="选择字典属性后加载；已有缓存会直接读取", foreground="#666")
        self.dictionary_status_label.grid(row=3, column=0, columnspan=5, sticky="w", pady=(5, 0))
        ttk.Button(editor, text="刷新缓存", command=lambda: self._begin_dictionary_load(force_refresh=True)).grid(row=3, column=5, padx=3, pady=(5, 0))

        ttk.Label(tab, text="高级 JSON（通常无需直接修改；提交时界面辅助字段会自动移除）").grid(row=5, column=0, columnspan=3, sticky="w", padx=5)
        json_tabs = ttk.Notebook(tab)
        json_tabs.grid(row=6, column=0, columnspan=3, sticky="nsew", padx=5, pady=(3, 5))
        simple_json_frame = ttk.Frame(json_tabs)
        complex_json_frame = ttk.Frame(json_tabs)
        simple_json_frame.columnconfigure(0, weight=1)
        simple_json_frame.rowconfigure(0, weight=1)
        complex_json_frame.columnconfigure(0, weight=1)
        complex_json_frame.rowconfigure(0, weight=1)
        json_tabs.add(simple_json_frame, text="普通属性 JSON")
        json_tabs.add(complex_json_frame, text="组合属性 JSON")
        self.attribute_text = tk.Text(simple_json_frame, wrap="none", font=("Consolas", 10), height=8)
        self.attribute_text.grid(row=0, column=0, sticky="nsew")
        self.attribute_text.insert("1.0", "[]")
        self.complex_attribute_text = tk.Text(complex_json_frame, wrap="none", font=("Consolas", 10), height=8)
        self.complex_attribute_text.grid(row=0, column=0, sticky="nsew")
        self.complex_attribute_text.insert("1.0", "[]")

    def _build_execute(self):
        tab = self.execute_tab
        tab.columnconfigure(1, weight=1)
        tab.rowconfigure(5, weight=1)
        self._entry(tab, "RFBS 仓库 ID", "warehouse_id", 0)
        self._entry(tab, "可售库存", "stock", 0, 2, default="1")
        ttk.Button(tab, text="读取仓库", command=lambda: self._run(self._load_warehouses)).grid(row=1, column=0, padx=5, pady=5, sticky="w")
        self.warehouse_combo = ttk.Combobox(tab, state="readonly", width=80)
        self.warehouse_combo.grid(row=1, column=1, columnspan=2, sticky="ew", padx=5)
        self.warehouse_combo.bind("<<ComboboxSelected>>", lambda _event: self._warehouse_selected())
        action_frame = ttk.Frame(tab)
        action_frame.grid(row=2, column=0, columnspan=4, sticky="ew", pady=12)
        ttk.Label(action_frame, text="当前店铺：").pack(side=tk.LEFT, padx=(5, 0))
        ttk.Label(action_frame, textvariable=self._var("ozon_shop_name"), foreground="#245a8d").pack(side=tk.LEFT, padx=(0, 12))
        self.draft_button = ttk.Button(action_frame, text="仅生成草稿（安全）", command=lambda: self._run(self._save_draft))
        self.draft_button.pack(side=tk.LEFT, padx=5)
        self.submit_button = ttk.Button(action_frame, text="提交 Ozon 并写 RFBS 库存", command=self._confirm_submit)
        self.submit_button.pack(side=tk.LEFT, padx=5)
        self.workspace_button = ttk.Button(action_frame, text="保存当前工作", command=self._manual_save_workspace)
        self.workspace_button.pack(side=tk.LEFT, padx=5)
        ttk.Label(tab, text="真实提交前必须先检查草稿；品牌默认无品牌，AI 不会伪造页面缺失的尺寸、材质、认证等事实。", foreground="#a33").grid(row=3, column=0, columnspan=4, sticky="w", padx=5)
        ttk.Label(tab, text="运行日志").grid(row=4, column=0, sticky="w", padx=5)
        self.log_text = tk.Text(tab, state=tk.DISABLED, wrap="word")
        self.log_text.grid(row=5, column=0, columnspan=4, sticky="nsew", padx=5, pady=5)

    def _config(self):
        selected_shop_id = str(self.vars.get("ozon_shop_id").get() if self.vars.get("ozon_shop_id") else "").strip()
        if selected_shop_id not in self.ozon_shops:
            selected_shop_id = next(iter(self.ozon_shops), "")
        return {
            "ozon": {
                "selected_shop_id": selected_shop_id,
                "shops": [dict(shop) for shop in self.ozon_shops.values()],
            },
            "image": {"api_url": self.vars["image_api_url"].get().strip(), "api_key": self.vars["image_api_key"].get().strip(), "model": self.vars["image_model"].get().strip()},
            "analysis": {"api_url": self.vars["analysis_api_url"].get().strip(), "api_key": self.vars["analysis_api_key"].get().strip(), "model": self.vars["analysis_model"].get().strip()},
            "text": {"api_url": self.vars["text_api_url"].get().strip(), "api_key": self.vars["text_api_key"].get().strip(), "model": self.vars["text_model"].get().strip()},
            "oss": {
                "region": self.vars["oss_region"].get().strip(), "endpoint": self.vars["oss_endpoint"].get().strip(),
                "bucket": self.vars["oss_bucket"].get().strip(), "public_base_url": self.vars["oss_public_base_url"].get().strip(),
                "access_key_id": self.vars["oss_access_key_id"].get().strip(), "access_key_secret": self.vars["oss_access_key_secret"].get().strip(),
                "object_prefix": self.vars["oss_object_prefix"].get().strip(),
            },
            "wb": {
                "token": self.vars["wb_token"].get().strip(),
                "sandbox": self.vars["wb_sandbox"].get().strip() == "1",
                "seller_name": (
                    self.vars["wb_seller_name"].get().strip()
                    if "wb_seller_name" in self.vars else ""
                ),
                "seller_id": (
                    self.vars["wb_seller_id"].get().strip()
                    if "wb_seller_id" in self.vars else ""
                ),
                "sales_commission_percent": (
                    self.vars["wb_sales_commission_percent"].get().strip()
                    if "wb_sales_commission_percent" in self.vars else ""
                ),
            },
        }

    def _analysis_config(self):
        cfg = self._config()
        return {
            key: cfg["analysis"].get(key) or cfg["text"].get(key)
            for key in ("api_url", "api_key", "model")
        }

    @staticmethod
    def _ozon_shops_from_config(ozon_config: dict) -> tuple[dict[str, dict], str]:
        ozon_config = ozon_config if isinstance(ozon_config, dict) else {}
        raw_shops = ozon_config.get("shops")
        profiles: dict[str, dict] = {}
        if isinstance(raw_shops, list):
            for index, raw in enumerate(raw_shops):
                if not isinstance(raw, dict):
                    continue
                client_id = str(raw.get("client_id") or "").strip()
                api_key = str(raw.get("api_key") or "").strip()
                if not client_id and not api_key:
                    continue
                fallback_key = client_id or f"profile-{index}"
                shop_id = str(raw.get("id") or "").strip() or uuid.uuid5(
                    uuid.NAMESPACE_URL, "ozon-shop:" + fallback_key,
                ).hex
                if shop_id in profiles:
                    shop_id = uuid.uuid5(
                        uuid.NAMESPACE_URL, f"ozon-shop:{fallback_key}:{index}",
                    ).hex
                profiles[shop_id] = {
                    "id": shop_id,
                    "name": str(raw.get("name") or f"店铺 {client_id or index + 1}").strip(),
                    "client_id": client_id,
                    "api_key": api_key,
                    "proxy_url": str(raw.get("proxy_url") or "").strip(),
                }
        elif any(str(ozon_config.get(key) or "").strip() for key in ("client_id", "api_key", "proxy_url")):
            shop_id = "legacy-default"
            profiles[shop_id] = {
                "id": shop_id,
                "name": "默认店铺",
                "client_id": str(ozon_config.get("client_id") or "").strip(),
                "api_key": str(ozon_config.get("api_key") or "").strip(),
                "proxy_url": str(ozon_config.get("proxy_url") or "").strip(),
            }
        selected_shop_id = str(ozon_config.get("selected_shop_id") or "").strip()
        if selected_shop_id not in profiles:
            selected_shop_id = next(iter(profiles), "")
        return profiles, selected_shop_id

    @staticmethod
    def _ozon_shop_display(shop: dict) -> str:
        return f"{shop.get('name') or '未命名店铺'} | Client-Id={shop.get('client_id') or '未填写'}"

    def _refresh_ozon_shop_choices(self):
        self.ozon_shop_display_by_id = {
            shop_id: self._ozon_shop_display(shop) for shop_id, shop in self.ozon_shops.items()
        }
        self.ozon_shop_id_by_display = {
            display: shop_id for shop_id, display in self.ozon_shop_display_by_id.items()
        }
        choices = list(self.ozon_shop_id_by_display)
        if getattr(self, "settings_shop_combo", None) is not None:
            self.settings_shop_combo.configure(values=choices)
        if getattr(self, "work_shop_combo", None) is not None:
            self.work_shop_combo.configure(values=choices)

    def _activate_ozon_shop(self, shop_id: str, *, sync_workbench: bool):
        shop = self.ozon_shops.get(str(shop_id or ""))
        if not shop:
            raise ValueError("所选 Ozon 店铺配置不存在，请到“服务配置”重新选择或保存店铺")
        display = self.ozon_shop_display_by_id.get(shop["id"], self._ozon_shop_display(shop))
        for key, value in (
            ("ozon_shop_id", shop["id"]), ("ozon_shop_display", display),
            ("ozon_shop_name", shop["name"]), ("ozon_client_id", shop["client_id"]),
            ("ozon_api_key", shop["api_key"]), ("ozon_proxy_url", shop["proxy_url"]),
        ):
            self._var(key).set(str(value))
        if getattr(self, "settings_shop_combo", None) is not None:
            self.settings_shop_combo.set(display)
        if sync_workbench:
            self._job_var("ozon_shop_id").set(shop["id"])
            self._job_var("ozon_shop_name").set(shop["name"])
            self._job_var("ozon_shop_display").set(display)
            if getattr(self, "work_shop_combo", None) is not None:
                self.work_shop_combo.set(display)

    def _clear_warehouse_after_shop_change(self):
        if str(getattr(self, "active_job_id", "") or ""):
            return
        if self.vars.get("warehouse_id"):
            self.vars["warehouse_id"].set("")
        self.warehouse_by_display = {}
        if getattr(self, "warehouse_combo", None) is not None:
            self.warehouse_combo.configure(values=[])
            self.warehouse_combo.set("")

    def _settings_shop_selected(self):
        display = self.settings_shop_combo.get().strip()
        shop_id = self.ozon_shop_id_by_display.get(display, "")
        if not shop_id:
            return
        old_work_shop_id = self._job_var("ozon_shop_id").get().strip()
        self._activate_ozon_shop(shop_id, sync_workbench=True)
        if old_work_shop_id and old_work_shop_id != shop_id:
            self._job_var("warehouse_id").set("")
            self._clear_warehouse_after_shop_change()

    def _work_shop_selected(self):
        display = self.work_shop_combo.get().strip()
        shop_id = self.ozon_shop_id_by_display.get(display, "")
        if not shop_id:
            return
        old_shop_id = self._job_var("ozon_shop_id").get().strip()
        shop = self.ozon_shops[shop_id]
        self._job_var("ozon_shop_id").set(shop_id)
        self._job_var("ozon_shop_name").set(shop["name"])
        if old_shop_id and old_shop_id != shop_id:
            self._job_var("warehouse_id").set("")
            self._clear_warehouse_after_shop_change()
        self._activate_ozon_shop(shop_id, sync_workbench=False)

    def _new_ozon_shop(self):
        for key in (
            "ozon_shop_id", "ozon_shop_display", "ozon_shop_name",
            "ozon_client_id", "ozon_api_key", "ozon_proxy_url",
        ):
            self._var(key).set("")
        self.settings_shop_combo.set("")

    def _upsert_current_ozon_shop(self) -> str:
        name = self.vars["ozon_shop_name"].get().strip()
        client_id = self.vars["ozon_client_id"].get().strip()
        api_key = self.vars["ozon_api_key"].get().strip()
        proxy_url = self.vars["ozon_proxy_url"].get().strip()
        if not name:
            raise ValueError("请填写便于识别的店铺名称")
        if not client_id or not api_key:
            raise ValueError("店铺 Client-Id 和 Api-Key 不能为空")
        OzonApi(client_id, api_key, proxy_url=proxy_url)
        shop_id = self.vars.get("ozon_shop_id").get().strip() if self.vars.get("ozon_shop_id") else ""
        for existing_id, profile in self.ozon_shops.items():
            if existing_id != shop_id and str(profile.get("client_id") or "").strip() == client_id:
                raise ValueError(f"Client-Id {client_id} 已保存在店铺“{profile.get('name')}”中")
        old_work_shop_id = self.job_form_vars.get("ozon_shop_id").get().strip() if self.job_form_vars.get("ozon_shop_id") else ""
        if not shop_id:
            shop_id = uuid.uuid4().hex
        self.ozon_shops[shop_id] = {
            "id": shop_id, "name": name, "client_id": client_id,
            "api_key": api_key, "proxy_url": proxy_url,
        }
        self._refresh_ozon_shop_choices()
        self._activate_ozon_shop(shop_id, sync_workbench=True)
        if old_work_shop_id and old_work_shop_id != shop_id:
            self._job_var("warehouse_id").set("")
            self._clear_warehouse_after_shop_change()
        return shop_id

    def _save_current_ozon_shop(self):
        try:
            shop_id = self._upsert_current_ozon_shop()
            atomic_write_json(CONFIG_PATH, self._config())
        except Exception as error:
            messagebox.showerror("店铺保存失败", str(error))
            return
        messagebox.showinfo("店铺已保存", f"已保存：{self.ozon_shops[shop_id]['name']}\nClient-Id：{self.ozon_shops[shop_id]['client_id']}")

    def _delete_current_ozon_shop(self):
        shop_id = self.vars.get("ozon_shop_id").get().strip() if self.vars.get("ozon_shop_id") else ""
        shop = self.ozon_shops.get(shop_id)
        if not shop:
            messagebox.showwarning("未选择店铺", "请先选择要删除的已保存店铺")
            return
        active_jobs = [
            job for job in getattr(self, "auto_jobs", {}).values()
            if str((job.get("inputs") or {}).get("ozon_shop_id") or "") == shop_id
            and str(job.get("status") or "") in {"queued", "running", "stopping"}
        ]
        if active_jobs:
            messagebox.showwarning(
                "店铺仍有任务",
                f"店铺“{shop['name']}”仍有 {len(active_jobs)} 个排队、运行或停止中的任务，不能删除。请先停止并处理这些任务。",
            )
            return
        if not messagebox.askyesno(
            "删除店铺",
            f"确定删除店铺“{shop['name']}”吗？\nClient-Id：{shop['client_id']}\n历史任务不会被删除，但以后重试时需要重新绑定店铺。",
        ):
            return
        self.ozon_shops.pop(shop_id, None)
        self._refresh_ozon_shop_choices()
        next_shop_id = next(iter(self.ozon_shops), "")
        if next_shop_id:
            self._activate_ozon_shop(next_shop_id, sync_workbench=True)
            self._job_var("warehouse_id").set("")
            self._clear_warehouse_after_shop_change()
        else:
            self._new_ozon_shop()
            self._job_var("ozon_shop_id").set("")
            self._job_var("ozon_shop_name").set("")
            self._job_var("ozon_shop_display").set("")
            self.work_shop_combo.set("")
            self._job_var("warehouse_id").set("")
            self._clear_warehouse_after_shop_change()
        atomic_write_json(CONFIG_PATH, self._config())

    def _save_config(self):
        try:
            selected_shop_id = self.vars.get("ozon_shop_id").get().strip() if self.vars.get("ozon_shop_id") else ""
            has_shop_fields = any(
                self.vars[key].get().strip()
                for key in ("ozon_shop_name", "ozon_client_id", "ozon_api_key", "ozon_proxy_url")
            )
            if selected_shop_id or has_shop_fields:
                self._upsert_current_ozon_shop()
            atomic_write_json(CONFIG_PATH, self._config())
        except Exception as error:
            messagebox.showerror("配置保存失败", str(error))
            return
        messagebox.showinfo("完成", f"配置已保存：{CONFIG_PATH}")

    def _load_config(self):
        cfg = load_json(CONFIG_PATH, {})
        mapping = {
            "image_api_url": ("image", "api_url"), "image_api_key": ("image", "api_key"), "image_model": ("image", "model"),
            "analysis_api_url": ("analysis", "api_url"), "analysis_api_key": ("analysis", "api_key"), "analysis_model": ("analysis", "model"),
            "text_api_url": ("text", "api_url"), "text_api_key": ("text", "api_key"), "text_model": ("text", "model"),
            "oss_region": ("oss", "region"), "oss_endpoint": ("oss", "endpoint"), "oss_bucket": ("oss", "bucket"),
            "oss_public_base_url": ("oss", "public_base_url"), "oss_access_key_id": ("oss", "access_key_id"),
            "oss_access_key_secret": ("oss", "access_key_secret"), "oss_object_prefix": ("oss", "object_prefix"),
        }
        for variable, path in mapping.items():
            value = cfg.get(path[0], {}).get(path[1])
            if value is not None:
                self.vars[variable].set(str(value))
        wb_config = cfg.get("wb") if isinstance(cfg.get("wb"), dict) else {}
        self.vars["wb_token"].set(str(wb_config.get("token") or ""))
        self.vars["wb_sandbox"].set("1" if wb_config.get("sandbox") else "0")
        self._var("wb_seller_name").set(str(wb_config.get("seller_name") or ""))
        self._var("wb_seller_id").set(str(wb_config.get("seller_id") or ""))
        self._var("wb_sales_commission_percent").set(
            str(wb_config.get("sales_commission_percent") or "")
        )
        self._refresh_wb_config_state()
        self.ozon_shops, selected_shop_id = self._ozon_shops_from_config(cfg.get("ozon", {}))
        self._refresh_ozon_shop_choices()
        if selected_shop_id:
            self._activate_ozon_shop(selected_shop_id, sync_workbench=True)
        else:
            self._new_ozon_shop()

    def _workspace_payload(self):
        fields = {
            key: self.vars[key].get()
            for key in WORKSPACE_FIELD_KEYS if key in self.vars
        }
        return {
            "version": 2,
            "saved_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            "fields": fields,
            "description": self.description_text.get("1.0", tk.END).strip(),
            "reference": export_reference(self.reference),
            "source_images": list(self.source_images),
            "local_images": list(self.local_images),
            "uploaded_urls": list(self.uploaded_urls),
            "primary_reference_index": int(self.primary_reference_index),
            "category_display": self.vars["category_display"].get(),
            "warehouse_display": self.warehouse_combo.get(),
            "attribute_definitions": self.attribute_definitions,
            "attribute_fact_suggestions": {
                str(key): list(value) for key, value in self.attribute_fact_suggestions.items()
            },
            "vision_analysis": getattr(self, "vision_analysis", {}),
            "attributes_json": self.attribute_text.get("1.0", tk.END).strip() or "[]",
            "complex_attributes_json": self.complex_attribute_text.get("1.0", tk.END).strip() or "[]",
        }

    def _save_workspace(self):
        atomic_write_json(WORKSPACE_PATH, self._workspace_payload())
        self._workspace_save_error = ""

    def _save_workspace_quietly(self):
        try:
            self._save_workspace()
        except Exception as error:
            message = str(error) or type(error).__name__
            if message != self._workspace_save_error:
                self._workspace_save_error = message
                self._log("自动保存当前工作失败：" + message)

    def _manual_save_workspace(self):
        try:
            self._save_workspace()
        except Exception as error:
            messagebox.showerror("保存失败", str(error))
            return
        messagebox.showinfo("已保存", f"当前商品、图片列表、类目和属性已保存。\n{WORKSPACE_PATH}")

    def _autosave_workspace(self):
        if self._closing:
            return
        self._save_workspace_quietly()
        self.root.after(15000, self._autosave_workspace)

    @staticmethod
    def _reference_from_dict(value):
        value = value if isinstance(value, dict) else {}
        return ReferenceProduct(
            str(value.get("source_url") or ""),
            str(value.get("title") or ""),
            str(value.get("description") or ""),
            [str(item) for item in value.get("images", []) if str(item).strip()],
            {str(key): str(item) for key, item in (value.get("properties") or {}).items()},
            [str(item) for item in value.get("tags", []) if str(item).strip()],
        )

    def _restore_workspace_payload(self, state: dict):
        fields = state.get("fields") if isinstance(state.get("fields"), dict) else {}
        if "tags" in fields:
            fields = dict(fields)
            fields["tags"] = format_ozon_hashtags(fields["tags"])
        for key in WORKSPACE_FIELD_KEYS:
            if key in fields and key in self.vars:
                self.vars[key].set(str(fields[key]))
        self.description_text.delete("1.0", tk.END)
        self.description_text.insert("1.0", str(state.get("description") or ""))
        self.reference = self._reference_from_dict(state.get("reference"))
        self.source_images = [
            str(path) for path in state.get("source_images", []) if Path(str(path)).is_file()
        ]
        self.local_images = [
            str(path) for path in state.get("local_images", []) if Path(str(path)).is_file()
        ]
        self.uploaded_urls = [
            str(url) for url in state.get("uploaded_urls", []) if str(url).startswith("https://")
        ]
        self.image_list.delete(0, tk.END)
        for path in self.local_images:
            self.image_list.insert(tk.END, path)
        try:
            selected_index = int(state.get("primary_reference_index") or 0)
        except (TypeError, ValueError):
            selected_index = 0
        self.primary_reference_index = selected_index if 0 <= selected_index < len(self.local_images) else 0
        if self.local_images:
            self.image_list.selection_set(self.primary_reference_index)
        category_display = str(fields.get("category_display") or state.get("category_display") or "")
        if category_display:
            category_values = list(self.category_displays)
            if category_display not in category_values:
                category_values.append(category_display)
            self.category_combo.configure(values=category_values)
            self.category_combo.set(category_display)
        warehouse_display = str(state.get("warehouse_display") or "")
        if warehouse_display:
            self.warehouse_combo.configure(values=[warehouse_display])
            self.warehouse_combo.set(warehouse_display)
        definitions = state.get("attribute_definitions") or []
        self.attribute_definitions = [item for item in definitions if isinstance(item, dict) and item.get("id")]
        self.attribute_definition_by_id = {int(item["id"]): item for item in self.attribute_definitions}
        suggestions = state.get("attribute_fact_suggestions") or {}
        self.attribute_fact_suggestions = {}
        for raw_id, raw_value in suggestions.items():
            if isinstance(raw_value, (list, tuple)) and len(raw_value) >= 2:
                try:
                    self.attribute_fact_suggestions[int(raw_id)] = (str(raw_value[0]), str(raw_value[1]))
                except (TypeError, ValueError):
                    pass
        analysis = state.get("vision_analysis")
        self.vision_analysis = analysis if isinstance(analysis, dict) else {}
        self.attribute_text.delete("1.0", tk.END)
        self.attribute_text.insert("1.0", str(state.get("attributes_json") or "[]"))
        self.complex_attribute_text.delete("1.0", tk.END)
        self.complex_attribute_text.insert("1.0", str(state.get("complex_attributes_json") or "[]"))
        if self.attribute_definitions:
            self._refresh_attribute_tree()
        elif self.vars["category_id"].get().strip():
            self.required_label.configure(text="已恢复类目和属性 JSON；提交前请重新读取实时属性进行校验")

    def _restore_latest_draft(self) -> bool:
        drafts = sorted(OUTPUT_DIR.glob("draft-*.json"), key=lambda path: path.stat().st_mtime, reverse=True)
        if not drafts:
            return False
        payload = load_json(drafts[0], {})
        item = payload.get("item") if isinstance(payload.get("item"), dict) else {}
        content = payload.get("content") if isinstance(payload.get("content"), dict) else {}
        reference = payload.get("source") if isinstance(payload.get("source"), dict) else {}
        tags = content.get("tags_ru") or []
        fields = {
            "source_url": reference.get("source_url", ""),
            "title": content.get("title_ru") or item.get("name", ""),
            "tags": format_ozon_hashtags(tags),
            "price": item.get("price", ""), "old_price": item.get("old_price", "0"),
            "currency_code": item.get("currency_code", "RUB"),
            "weight": item.get("weight", ""), "weight_unit": item.get("weight_unit", "g"),
            "depth": item.get("depth", ""), "width": item.get("width", ""), "height": item.get("height", ""),
            "dimension_unit": item.get("dimension_unit", "mm"), "vat": item.get("vat", "0"),
            "offer_id": item.get("offer_id", ""),
            "category_id": item.get("description_category_id", ""), "type_id": item.get("type_id", ""),
        }
        uploaded_urls = []
        if str(item.get("primary_image") or "").startswith("https://"):
            uploaded_urls.append(str(item["primary_image"]))
        uploaded_urls.extend(str(url) for url in item.get("images", []) if str(url).startswith("https://"))
        state = {
            "fields": fields,
            "description": content.get("description_ru", ""),
            "reference": reference,
            "uploaded_urls": uploaded_urls,
            "attributes_json": json.dumps(item.get("attributes", []), ensure_ascii=False, indent=2),
            "complex_attributes_json": json.dumps(item.get("complex_attributes", []), ensure_ascii=False, indent=2),
        }
        self._restore_workspace_payload(state)
        self._log("未找到自动保存文件，已从最近草稿恢复：" + str(drafts[0]))
        self._save_workspace_quietly()
        return True

    def _restore_workspace(self):
        if WORKSPACE_PATH.is_file():
            state = load_json(WORKSPACE_PATH, {})
            if isinstance(state, dict) and state:
                self._restore_workspace_payload(state)
                saved_at = str(state.get("saved_at") or "未知时间")
                self._log(f"已恢复上次工作内容（保存时间：{saved_at}）")
                return
        self._restore_latest_draft()

    def _on_close(self):
        self._closing = True
        try:
            self._save_workspace()
        except Exception as error:
            if not messagebox.askyesno("当前工作保存失败", f"{error}\n\n仍然退出程序吗？"):
                self._closing = False
                self.root.after(15000, self._autosave_workspace)
                return
        self.root.destroy()

    def _ozon_shop_for_api(self, shop_id: str | None = None) -> dict:
        resolved_shop_id = shop_id
        if resolved_shop_id is None:
            active_job_id = str(getattr(self, "active_job_id", "") or "")
            active_job = getattr(self, "auto_jobs", {}).get(active_job_id, {})
            if active_job:
                resolved_shop_id = str((active_job.get("inputs") or {}).get("ozon_shop_id") or "").strip()
        if not resolved_shop_id:
            variable = self.vars.get("ozon_shop_id")
            resolved_shop_id = variable.get().strip() if variable else ""
        if resolved_shop_id:
            shop = self.ozon_shops.get(resolved_shop_id)
            if not shop:
                raise ValueError(
                    "任务绑定的 Ozon 店铺配置已不存在。请在“修复选中任务”中重新选择店铺后继续"
                )
            return shop
        temporary = {
            "id": "", "name": self.vars["ozon_shop_name"].get().strip() or "未保存店铺",
            "client_id": self.vars["ozon_client_id"].get().strip(),
            "api_key": self.vars["ozon_api_key"].get().strip(),
            "proxy_url": self.vars["ozon_proxy_url"].get().strip(),
        }
        if not temporary["client_id"] or not temporary["api_key"]:
            raise ValueError("请先在“服务配置”保存并选择一个 Ozon 店铺")
        return temporary

    def _api(self, shop_id: str | None = None):
        shop = self._ozon_shop_for_api(shop_id)
        return OzonApi(
            shop["client_id"], shop["api_key"], proxy_url=shop.get("proxy_url", ""),
            catalog_language="ZH_HANS",
        )

    def _test_ozon_connection(self):
        shop = self._ozon_shop_for_api()
        categories = flatten_categories(self._api(shop.get("id") or None).category_tree())
        if not categories:
            raise RuntimeError("已连接 Ozon，但类目树为空；请检查 API 权限")
        proxy = str(shop.get("proxy_url") or "").strip()
        route = f"代理 {proxy}" if proxy else "直连"
        shop_name = str(shop.get("name") or "未命名店铺")
        self._log(f"店铺“{shop_name}” Ozon Seller API 连接成功（{route}），读取到 {len(categories)} 个可用商品类型")
        self.root.after(0, lambda: messagebox.showinfo(
            "连接成功", f"店铺：{shop_name}\nOzon Seller API 可访问。\n方式：{route}\n商品类型：{len(categories)}",
        ))

    def _run(self, operation):
        if self.busy:
            messagebox.showwarning("请稍候", "已有任务正在运行")
            return
        self.busy = True
        self._log("开始：" + operation.__name__)
        def worker():
            succeeded = False
            try:
                result = operation()
                succeeded = True
                self._log("完成：" + operation.__name__)
                return result
            except Exception as error:
                # Python clears an exception variable after leaving ``except``.
                # Capture the text now so Tkinter's delayed callback can still use it.
                error_message = str(error) or f"{type(error).__name__}: 未提供错误详情"
                self._log("错误：" + error_message)
                self.root.after(0, lambda message=error_message: messagebox.showerror("执行失败", message))
            finally:
                self.busy = False
                if succeeded:
                    # 业务方法会先把界面更新排入 Tk 事件队列；随后保存即可捕获更新结果。
                    self.root.after(0, self._save_workspace_quietly)
        threading.Thread(target=worker, daemon=True).start()

    def _ui_call(self, callback, timeout=30):
        """Run one callback on Tk's thread and wait for it from a worker."""
        if threading.current_thread() is threading.main_thread():
            return callback()
        completed = threading.Event()
        result = {}
        def invoke():
            try:
                result["value"] = callback()
            except Exception as error:
                result["error"] = error
            finally:
                completed.set()
        self.root.after(0, invoke)
        if not completed.wait(timeout):
            raise RuntimeError("等待界面更新超时")
        if "error" in result:
            raise result["error"]
        return result.get("value")

    def _set_auto_progress(self, value: int, text: str):
        self.auto_progress_value = max(0, min(9, int(value)))
        def update():
            self.auto_progress.configure(value=self.auto_progress_value)
            self.auto_status_var.set(text)
        if threading.current_thread() is threading.main_thread():
            update()
        else:
            self.root.after(0, update)
        self._log(text)

    @staticmethod
    def _is_retryable_auto_error(error: Exception) -> bool:
        if isinstance(error, (AutoJobCancelled, ValueError, FileNotFoundError, PermissionError)):
            return False
        message = str(error).casefold()
        if re.search(r"http\s+(400|401|403|404|405|409|413|415|422)", message):
            return False
        if any(marker in message for marker in (
            "不能为空", "必须大于", "不能小于", "仍缺少 ozon 必填属性",
            "请先填写", "请先选择", "配置缺少", "密钥和模型",
        )):
            return False
        return True

    def _retry_step(
        self, label: str, operation, *, attempts=3, delay_seconds=2, cancel_check=None,
    ):
        last_error = None
        for attempt in range(1, max(1, int(attempts)) + 1):
            if cancel_check:
                cancel_check()
            try:
                return operation()
            except Exception as error:
                last_error = error
                if attempt >= attempts or not self._is_retryable_auto_error(error):
                    raise
                delay = min(15, max(0, delay_seconds) * (2 ** (attempt - 1)))
                self._log(f"{label}第 {attempt} 次失败：{error}；{delay} 秒后自动重试")
                if delay:
                    deadline = time.monotonic() + delay
                    while time.monotonic() < deadline:
                        if cancel_check:
                            cancel_check()
                        time.sleep(min(0.25, max(0, deadline - time.monotonic())))
        raise last_error or RuntimeError(label + "失败")

    def _initialize_job_form_from_workspace(self):
        for key, variable in self.job_form_vars.items():
            if key == "model_name":
                continue
            if key in self.vars:
                value = self.vars[key].get().strip()
                if value:
                    variable.set(value)
        self.job_form_vars["model_name"].set("")
        shop_variable = self.job_form_vars.get("ozon_shop_id")
        shop_id = shop_variable.get().strip() if shop_variable else ""
        ozon_shops = getattr(self, "ozon_shops", {})
        if shop_id in ozon_shops:
            shop = ozon_shops[shop_id]
            if self.job_form_vars.get("ozon_shop_name"):
                self.job_form_vars["ozon_shop_name"].set(shop["name"])
            display = self.ozon_shop_display_by_id.get(shop_id, self._ozon_shop_display(shop))
            if self.job_form_vars.get("ozon_shop_display"):
                self.job_form_vars["ozon_shop_display"].set(display)
            if getattr(self, "work_shop_combo", None) is not None:
                self.work_shop_combo.set(display)
        for key in ("category_id", "type_id"):
            if key in self.vars and self.vars[key].get().strip():
                self._job_var(key).set(self.vars[key].get().strip())
        self._job_category_selected()
        listing_mode_variable = self.job_form_vars.get("listing_mode")
        if (
            listing_mode_variable is not None
            and self._local_listing_enabled({
                "listing_mode": listing_mode_variable.get(),
            })
            and not getattr(self, "job_form_image_paths", [])
            and getattr(self, "local_images", [])
        ):
            self._set_job_local_image_paths(self.local_images)
        if listing_mode_variable is not None:
            self._on_listing_mode_changed()
        if getattr(self, "price_breakdown_var", None) is not None:
            self._on_fbp_pricing_changed()

    def _job_category_selected(self):
        display_var = self.job_form_vars.get("category_display")
        display = display_var.get().strip() if display_var else ""
        category = self.category_by_display.get(display)
        if category:
            self._job_var("category_id").set(str(category["description_category_id"]))
            self._job_var("type_id").set(str(category["type_id"]))
            self.work_category_id_label.configure(
                text=f"已选择类目 ID：{category['description_category_id']} / 类型 ID：{category['type_id']}"
            )
            return category
        saved_category_id = self.job_form_vars.get("category_id")
        saved_type_id = self.job_form_vars.get("type_id")
        if saved_category_id and saved_type_id and saved_category_id.get() and saved_type_id.get():
            self.work_category_id_label.configure(
                text=f"已保存类目 ID：{saved_category_id.get()} / 类型 ID：{saved_type_id.get()}；刷新类目后请确认"
            )
        else:
            self.work_category_id_label.configure(text="尚未选择实时类目")
        return None

    def _job_inputs_from_form(self) -> dict:
        values = {}
        for key in JOB_INPUT_KEYS:
            if key in self.job_form_vars:
                values[key] = self.job_form_vars[key].get().strip()
            elif key in self.vars:
                values[key] = self.vars[key].get().strip()
            else:
                values[key] = ""
        category = self.category_by_display.get(values.get("category_display", ""))
        if category:
            values["category_id"] = str(category["description_category_id"])
            values["type_id"] = str(category["type_id"])
        shop = getattr(self, "ozon_shops", {}).get(str(values.get("ozon_shop_id") or ""))
        if shop:
            values["ozon_shop_name"] = shop["name"]
        if self._local_listing_enabled(values):
            values["source_url"] = ""
            values["local_image_paths"] = list(self.job_form_image_paths)
        else:
            values["local_image_paths"] = []
        return values

    def _validate_job_inputs(self, values: dict):
        local_mode = self._local_listing_enabled(values)
        if not local_mode and not str(values.get("source_url") or "").strip():
            raise ValueError("请填写 Ozon 链接或商品 ID")
        if local_mode:
            local_paths = [
                str(path) for path in (values.get("local_image_paths") or [])
                if str(path).strip()
            ]
            if not local_paths:
                raise ValueError("本地新品模式请至少选择 1 张成品图片")
            if len(local_paths) > 15:
                raise ValueError("Ozon 商品图片最多选择 15 张")
            missing_images = [path for path in local_paths if not Path(path).is_file()]
            if missing_images:
                raise ValueError("以下本地商品图片不存在：" + "、".join(missing_images[:3]))
        if not str(values.get("offer_id") or "").strip():
            raise ValueError("请填写上传货号（Offer ID）")
        model_name = str(values.get("model_name") or "").strip()
        if not model_name:
            raise ValueError("请人工填写型号名称；需要合并为同一商品卡片的变体必须填写完全相同的型号名称")
        if re.search(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]", model_name):
            raise ValueError("型号名称是 Ozon 自由文本属性，不能填写中文；请使用俄文、英文、数字或允许的符号")
        shop_id = str(values.get("ozon_shop_id") or "").strip()
        shop = self.ozon_shops.get(shop_id)
        if not shop:
            raise ValueError("请先在自动工作台选择一个已保存的 Ozon 上品店铺")
        if not str(shop.get("client_id") or "").strip() or not str(shop.get("api_key") or "").strip():
            raise ValueError(f"店铺“{shop.get('name') or shop_id}”缺少 Client-Id 或 Api-Key，请先完善配置")
        try:
            manual_commission = Decimal(str(values.get("sales_commission_percent") or "").strip())
        except (InvalidOperation, ValueError) as error:
            raise ValueError("请填写有效的 Ozon 标准佣金百分比") from error
        if not manual_commission.is_finite() or manual_commission <= 0 or manual_commission >= 100:
            raise ValueError("Ozon 标准佣金必须大于 0% 且小于 100%")
        if self._fbp_pricing_enabled(values) and manual_commission <= 1:
            raise ValueError("FBP 核价要求填写的标准佣金大于 1%")
        if self._fbp_pricing_enabled(values):
            values["label_fee"] = "0"
        pricing_fields = ("purchase_cost", "label_fee", "target_roi")
        if any(str(values.get(key) or "").strip() for key in pricing_fields):
            breakdown = self._job_price_breakdown(values)
            values["price"] = format(breakdown.price, "f")
        try:
            price = Decimal(str(values.get("price") or "").strip())
            old_price = Decimal(str(values.get("old_price") or "0").strip() or "0")
        except (InvalidOperation, ValueError) as error:
            raise ValueError("售价和折扣前价格必须是有效数字") from error
        if price <= 0:
            raise ValueError("售价必须大于 0")
        if old_price < 0 or (old_price and old_price <= price):
            raise ValueError("折扣前价格必须大于售价；不设置折扣时填写 0")
        physical_values = {}
        for key, label in (("net_weight", "商品重量"), ("weight", "毛重"), ("depth", "包装长度"), ("width", "包装宽度"), ("height", "包装高度")):
            try:
                number = float(values.get(key, ""))
            except (TypeError, ValueError) as error:
                raise ValueError(label + "必须是有效数字") from error
            if number <= 0:
                raise ValueError(label + "必须大于 0")
            physical_values[key] = number
        if physical_values["weight"] < physical_values["net_weight"]:
            raise ValueError("毛重（含包装）不能小于商品净重")
        display = str(values.get("category_display") or "").strip()
        category = self.category_by_display.get(display)
        if not category:
            raise ValueError("请先加载 Ozon 实时类目，并从下拉列表中人工选择一个类目")
        if not str(values.get("warehouse_id") or "").strip():
            raise ValueError("请先在“草稿与上架”页面选择 RFBS 仓库")
        try:
            stock = int(values.get("stock", ""))
        except (TypeError, ValueError) as error:
            raise ValueError("可售库存必须是整数") from error
        if stock < 0:
            raise ValueError("可售库存不能小于 0")
        watermark_path = str(values.get("watermark_path") or "").strip()
        if not local_mode and not Path(watermark_path).is_file():
            raise ValueError("请先在“商品与图片”页面选择有效的水印文件")
        if local_mode and watermark_path and not Path(watermark_path).is_file():
            raise ValueError("已填写的水印文件不存在；请重新选择或留空")
        return category

    def _snapshot_local_job_images(self, paths, offer_id: str, job_id: str) -> list[str]:
        source_paths = [Path(path) for path in paths][:15]
        if not source_paths:
            raise ValueError("本地新品模式请至少选择 1 张成品图片")
        for source in source_paths:
            if not source.is_file():
                raise FileNotFoundError("本地商品图片不存在：" + str(source))
        safe_offer = re.sub(r"[^A-Za-z0-9._#-]+", "_", str(offer_id or "local")).strip("._-") or "local"
        run_id = time.strftime("%Y%m%d-%H%M%S") + "-" + uuid.uuid4().hex[:6]
        output_dir = OUTPUT_DIR / "local-products" / safe_offer / f"{job_id}-{run_id}"
        output_dir.mkdir(parents=True, exist_ok=True)
        copied_paths = []
        for index, source in enumerate(source_paths, start=1):
            suffix = source.suffix.lower()
            if suffix not in {".jpg", ".jpeg", ".png", ".webp"}:
                raise ValueError("不支持的商品图片格式：" + str(source))
            destination = output_dir / f"{index:02d}{suffix}"
            shutil.copy2(source, destination)
            copied_paths.append(str(destination))
        self._log(f"本地新品图片已锁定副本 {len(copied_paths)} 张：{output_dir}")
        return copied_paths

    def _save_auto_jobs(self):
        with self.auto_job_lock:
            atomic_write_json(AUTO_JOBS_PATH, {
                "version": 1,
                "saved_at": time.strftime("%Y-%m-%d %H:%M:%S"),
                "order": list(self.auto_job_order),
                "jobs": self.auto_jobs,
            })

    def _load_auto_jobs(self):
        payload = load_json(AUTO_JOBS_PATH, {})
        jobs = payload.get("jobs") if isinstance(payload, dict) else {}
        order = payload.get("order") if isinstance(payload, dict) else []
        if not isinstance(jobs, dict):
            jobs = {}
        if not isinstance(order, list):
            order = []
        self.auto_jobs = {str(key): value for key, value in jobs.items() if isinstance(value, dict)}
        self.auto_job_order = [str(key) for key in order if str(key) in self.auto_jobs]
        for job_id in self.auto_jobs:
            if job_id not in self.auto_job_order:
                self.auto_job_order.append(job_id)
        changed = False
        selected_shop_id = self.vars.get("ozon_shop_id").get().strip() if self.vars.get("ozon_shop_id") else ""
        for job in self.auto_jobs.values():
            inputs = job.get("inputs") if isinstance(job.get("inputs"), dict) else {}
            if "listing_mode" not in inputs:
                inputs["listing_mode"] = "follow"
                job["inputs"] = inputs
                changed = True
            if "local_image_paths" not in inputs:
                inputs["local_image_paths"] = []
                job["inputs"] = inputs
                changed = True
            if "fbp_pricing" not in inputs:
                inputs["fbp_pricing"] = "0"
                job["inputs"] = inputs
                changed = True
            if "sales_commission_percent" not in inputs:
                inputs["sales_commission_percent"] = ""
                job["inputs"] = inputs
                changed = True
            for key, default in (("advertising_percent", "15"), ("cargo_loss_percent", "10")):
                if not str(inputs.get(key) or "").strip():
                    inputs[key] = default
                    job["inputs"] = inputs
                    changed = True
            if "supplier_1688_url" not in inputs:
                inputs["supplier_1688_url"] = ""
                job["inputs"] = inputs
                changed = True
            if selected_shop_id and not str(inputs.get("ozon_shop_id") or "").strip():
                inputs["ozon_shop_id"] = selected_shop_id
                inputs["ozon_shop_name"] = str(self.ozon_shops.get(selected_shop_id, {}).get("name") or "默认店铺")
                job["inputs"] = inputs
                changed = True
            if job.get("status") == "stopping":
                job["status"] = "cancelled"
                job["cancel_requested"] = False
                job["error"] = ""
                job["message"] = "程序关闭前已请求停止；任务保持为已停止"
                changed = True
            elif job.get("status") in {"running", "queued"}:
                job["status"] = "failed"
                job["cancel_requested"] = False
                job["error"] = "程序上次关闭时任务尚未完成；请点击修复或继续"
                changed = True
        if changed:
            self._save_auto_jobs()
        self._refresh_job_tree()

    def _refresh_job_tree(self):
        if threading.current_thread() is not threading.main_thread():
            self.root.after(0, self._refresh_job_tree)
            return
        self.job_tree.delete(*self.job_tree.get_children())
        status_names = {
            "queued": "排队中", "running": "运行中", "failed": "待修复", "completed": "已完成",
            "stopping": "停止中", "cancelled": "已停止",
        }
        for job_id in reversed(self.auto_job_order):
            job = self.auto_jobs.get(job_id, {})
            values = job.get("inputs") or {}
            source = str(values.get("source_url") or "")
            article = re.search(r"\d{6,}", source)
            local_mode = self._local_listing_enabled(values)
            product_source = (
                f"本地图片 {len(values.get('local_image_paths') or [])} 张"
                if local_mode else (article.group(0) if article else source)
            )
            self.job_tree.insert("", tk.END, iid=job_id, values=(
                values.get("ozon_shop_name", ""), "本地新品" if local_mode else "跟卖",
                values.get("offer_id", ""), values.get("model_name", ""), product_source,
                status_names.get(job.get("status"), str(job.get("status") or "")),
                f"{int(job.get('stage') or 0)}/7", str(job.get("error") or job.get("message") or "")[:240],
            ))

    def _enqueue_job_from_form(self):
        try:
            inputs = self._job_inputs_from_form()
            category = self._validate_job_inputs(inputs)
        except Exception as error:
            messagebox.showerror("录入内容不完整", str(error))
            return
        source_summary = (
            f"本地图片 {len(inputs.get('local_image_paths') or [])} 张"
            if self._local_listing_enabled(inputs) else inputs["source_url"]
        )
        summary = (
            f"上品模式：{'本地新品' if self._local_listing_enabled(inputs) else '跟卖'}\n"
            f"上品店铺：{inputs['ozon_shop_name']}\n货号：{inputs['offer_id']}\n"
            f"商品来源：{source_summary}\n"
            f"1688 采购链接：{inputs['supplier_1688_url'] or '未填写'}\n"
            f"型号名称：{inputs['model_name']}\n售价：{inputs['price']} RUB\n"
            f"核价模式：{'FBP（贴单费 0 元，佣金减 1 个百分点）' if self._fbp_pricing_enabled(inputs) else 'RFBS 标准核价'}\n"
            f"填写的标准佣金：{inputs['sales_commission_percent']}%\n"
            f"采购成本 / 贴单费 / 目标 ROI：{inputs['purchase_cost']} / {inputs['label_fee']} / {inputs['target_roi']}%\n"
            f"类目：{category['display']}\n\n"
            "加入后会在后台按顺序自动上架；你可以立即继续填写下一件商品。是否加入队列？"
        )
        if not messagebox.askyesno("加入自动上架队列", summary):
            return
        job_id = time.strftime("%Y%m%d%H%M%S") + "-" + uuid.uuid4().hex[:8]
        if self._local_listing_enabled(inputs):
            try:
                inputs["local_image_paths"] = self._snapshot_local_job_images(
                    inputs.get("local_image_paths") or [], inputs["offer_id"], job_id,
                )
            except Exception as error:
                messagebox.showerror("本地图片保存失败", str(error))
                return
        now = time.strftime("%Y-%m-%d %H:%M:%S")
        job = {
            "id": job_id, "created_at": now, "updated_at": now,
            "status": "queued", "stage": 0, "failed_stage": 0,
            "inputs": inputs, "state": {}, "error": "", "message": "等待运行",
            "task_id": 0, "import_completed": False, "stock_completed": False,
            "ledger_completed": False,
            "cancel_requested": False,
        }
        with self.auto_job_lock:
            self.auto_jobs[job_id] = job
            self.auto_job_order.append(job_id)
            self._save_auto_jobs()
        self.auto_job_queue.put(job_id)
        self._refresh_job_tree()
        self.job_form_vars["source_url"].set("")
        self.job_form_vars["supplier_1688_url"].set("")
        self.job_form_vars["offer_id"].set(f"AUTO-{time.strftime('%Y%m%d')}-{uuid.uuid4().hex[:6].upper()}")
        self.job_form_vars["model_name"].set("")
        if self._local_listing_enabled(inputs):
            self._clear_job_local_images()
        self.auto_status_var.set(f"已加入 {inputs['offer_id']}；现在可以填写下一件商品")
        self._start_auto_job_worker()

    def _start_auto_job_worker(self):
        with self.auto_job_lock:
            if self.auto_job_worker_running:
                return
            self.auto_job_worker_running = True
        threading.Thread(target=self._auto_job_worker, daemon=True).start()

    def _auto_job_worker(self):
        self.busy = True
        try:
            while True:
                try:
                    job_id = self.auto_job_queue.get_nowait()
                except queue.Empty:
                    break
                with self.auto_job_lock:
                    job = self.auto_jobs.get(job_id)
                    should_skip = not job or job.get("status") != "queued"
                    if not should_skip:
                        self.active_job_id = job_id
                        job["status"] = "running"
                        job["cancel_requested"] = False
                        job["error"] = ""
                        job["updated_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
                        self._save_auto_jobs()
                if should_skip:
                    self.auto_job_queue.task_done()
                    continue
                self._refresh_job_tree()
                try:
                    self._run_auto_job(job)
                    job["status"] = "completed"
                    job["stage"] = 7
                    job["message"] = "上架、RFBS 库存和 Excel 台账已完成"
                    job["error"] = ""
                except AutoJobCancelled as error:
                    job["status"] = "cancelled"
                    job["cancel_requested"] = False
                    job["error"] = ""
                    job["message"] = str(error) or "任务已按要求停止"
                    try:
                        job["state"] = self._ui_call(self._workspace_payload)
                    except Exception:
                        pass
                    self._log(
                        f"任务 {job.get('inputs', {}).get('offer_id')} 已按要求停止；"
                        "其他排队任务将继续运行"
                    )
                except Exception as error:
                    job["status"] = "failed"
                    job["failed_stage"] = min(7, int(job.get("stage") or 0) + 1)
                    job["error"] = str(error) or type(error).__name__
                    job["message"] = "点击“修复选中任务”修改后继续"
                    try:
                        job["state"] = self._ui_call(self._workspace_payload)
                    except Exception:
                        pass
                    self._log(f"任务 {job.get('inputs', {}).get('offer_id')} 已暂停待修复：{job['error']}")
                finally:
                    job["updated_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
                    self._save_auto_jobs()
                    self._refresh_job_tree()
                    self.auto_job_queue.task_done()
        finally:
            self.active_job_id = ""
            self.busy = False
            with self.auto_job_lock:
                self.auto_job_worker_running = False
            self.root.after(0, lambda: self.auto_status_var.set("队列当前已处理完；可继续加入产品或修复失败任务"))
            if not self.auto_job_queue.empty():
                self._start_auto_job_worker()

    def _reset_execution_for_job(self):
        self.reference = ReferenceProduct("", "")
        self.source_images = []
        self.local_images = []
        self.uploaded_urls = []
        self.primary_reference_index = 0
        self.image_list.delete(0, tk.END)
        self.description_text.delete("1.0", tk.END)
        self.vars["title"].set("")
        self.vars["tags"].set("")
        self.attribute_definitions = []
        self.attribute_definition_by_id = {}
        self.attribute_fact_suggestions = {}
        self.vision_analysis = {}
        self.attribute_text.delete("1.0", tk.END)
        self.attribute_text.insert("1.0", "[]")
        self.complex_attribute_text.delete("1.0", tk.END)
        self.complex_attribute_text.insert("1.0", "[]")
        self._refresh_attribute_tree()

    def _apply_job_context(self, job: dict):
        def apply():
            state = job.get("state") if isinstance(job.get("state"), dict) else {}
            if int(job.get("stage") or 0) > 0 and state:
                self._restore_workspace_payload(state)
            else:
                self._reset_execution_for_job()
            for key, value in (job.get("inputs") or {}).items():
                if key in self.vars:
                    self.vars[key].set(str(value))
            shop_id = str((job.get("inputs") or {}).get("ozon_shop_id") or "")
            if shop_id:
                self._activate_ozon_shop(shop_id, sync_workbench=False)
            display = str((job.get("inputs") or {}).get("category_display") or "")
            self.category_combo.set(display)
        self._ui_call(apply)

    def _checkpoint_auto_job(self, job: dict, stage: int, message: str):
        job["stage"] = int(stage)
        job["message"] = message
        job["state"] = self._ui_call(self._workspace_payload)
        job["updated_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
        self._save_auto_jobs()
        self._refresh_job_tree()

    @staticmethod
    def _check_auto_job_cancelled(job: dict):
        if not job.get("cancel_requested") and job.get("status") != "stopping":
            return
        task_id = int(job.get("task_id") or 0)
        if task_id:
            raise AutoJobCancelled(
                f"已停止选中任务的后续流程；Ozon 导入任务 {task_id} 已经提交，无法由程序撤回，"
                "RFBS 库存尚未继续写入。请到 Ozon 后台按任务 ID 或 Offer ID 处理"
            )
        raise AutoJobCancelled(
            "已停止选中任务；尚未向 Ozon 提交商品。可点击“修复选中任务”修改后继续"
        )

    def _run_auto_job(self, job: dict):
        cancel_check = lambda: self._check_auto_job_cancelled(job)
        cancel_check()
        self._apply_job_context(job)
        cancel_check()
        stage = int(job.get("stage") or 0)
        offer_id = str((job.get("inputs") or {}).get("offer_id") or "")
        local_mode = self._local_listing_enabled(job.get("inputs") or {})
        self.auto_pipeline_running = True
        try:
            if stage < 1:
                cancel_check()
                if local_mode:
                    self._set_auto_progress(1, f"{offer_id}：读取并分析本地新品图片")
                    self._retry_step(
                        "本地图片视觉分析",
                        lambda: self._prepare_local_job_images(job, analyze=True),
                        attempts=3, cancel_check=cancel_check,
                    )
                else:
                    self._set_auto_progress(1, f"{offer_id}：读取商品并下载高清原图")
                    self._retry_step(
                        "商品解析/原图下载", self._parse_url, attempts=3,
                        cancel_check=cancel_check,
                    )
                self._ui_call(lambda: None)
                self._checkpoint_auto_job(
                    job, 1, "本地图片视觉分析完成" if local_mode else "商品和原图完成",
                )
                cancel_check()
            if stage < 2:
                cancel_check()
                self._set_auto_progress(2, f"{offer_id}：生成俄文文案和标签")
                self._retry_step(
                    "文案生成", self._generate_copywriting, attempts=3,
                    cancel_check=cancel_check,
                )
                self._ui_call(lambda: None)
                self._checkpoint_auto_job(job, 2, "文案完成")
                cancel_check()
            if stage < 3:
                cancel_check()
                if local_mode:
                    self._set_auto_progress(3, f"{offer_id}：准备成品图片并按需添加水印")
                    self._retry_step(
                        "本地成品图片处理", self._prepare_local_images_for_upload,
                        attempts=2, delay_seconds=2, cancel_check=cancel_check,
                    )
                else:
                    self._set_auto_progress(3, f"{offer_id}：多图视觉分析、生成主图并加水印")
                    self._retry_step(
                        "视觉分析/主图生成", self._generate_images, attempts=2,
                        delay_seconds=4, cancel_check=cancel_check,
                    )
                self._ui_call(lambda: None)
                self._checkpoint_auto_job(job, 3, "图片处理完成")
                cancel_check()
            if stage < 4:
                cancel_check()
                self._set_auto_progress(4, f"{offer_id}：上传图片到 OSS")
                self._retry_step(
                    "OSS 图片上传", self._upload_images, attempts=3,
                    cancel_check=cancel_check,
                )
                self._checkpoint_auto_job(job, 4, "OSS 上传完成")
                cancel_check()
            if stage < 5:
                cancel_check()
                self._set_auto_progress(5, f"{offer_id}：填写当前类目属性")
                self._retry_step(
                    "类目属性填写", lambda: self._ai_fill_category_attributes(use_selected_category=True),
                    attempts=3, delay_seconds=3, cancel_check=cancel_check,
                )
                self._ui_call(lambda: None)
                self._checkpoint_auto_job(job, 5, "类目属性完成")
                cancel_check()
            if stage < 6:
                cancel_check()
                self._set_auto_progress(6, f"{offer_id}：校验并保存草稿")
                job["draft_path"] = str(self._save_draft(notify=False))
                self._checkpoint_auto_job(job, 6, "草稿校验完成")
                cancel_check()
            if stage < 7:
                cancel_check()
                self._set_auto_progress(7, f"{offer_id}：提交 Ozon、写入 RFBS 库存和 Excel 台账")
                job["submitted_path"] = str(self._submit(notify=False, auto_progress=True, job_record=job))
                self._checkpoint_auto_job(job, 7, "上架、库存和产品台账完成")
                cancel_check()
            self._set_auto_progress(9, f"{offer_id}：自动上架完成")
        finally:
            self.auto_pipeline_running = False

    def _selected_job(self):
        selected = self.job_tree.selection()
        return self.auto_jobs.get(selected[0]) if selected else None

    def _clear_finished_jobs(self):
        removable_statuses = {"completed", "cancelled", "failed"}
        with self.auto_job_lock:
            removable_ids = [
                job_id for job_id in self.auto_job_order
                if str((self.auto_jobs.get(job_id) or {}).get("status") or "") in removable_statuses
            ]
        if not removable_ids:
            messagebox.showinfo(
                "没有可清除的任务",
                "当前没有已完成、已停止或待修复的任务。排队中和运行中的任务会保留；如需清除，请先停止任务。",
            )
            return
        if not messagebox.askyesno(
            "清除已结束任务",
            f"将永久删除 {len(removable_ids)} 条已完成、已停止或待修复的任务记录及其断点。\n"
            "排队中、运行中和停止中的任务不会被删除。是否继续？",
        ):
            return
        with self.auto_job_lock:
            removable_ids = [
                job_id for job_id in removable_ids
                if str((self.auto_jobs.get(job_id) or {}).get("status") or "") in removable_statuses
            ]
            removable_set = set(removable_ids)
            for job_id in removable_ids:
                self.auto_jobs.pop(job_id, None)
            self.auto_job_order = [
                job_id for job_id in self.auto_job_order if job_id not in removable_set
            ]
            self._save_auto_jobs()
        self._refresh_job_tree()
        self.auto_status_var.set(f"已清除 {len(removable_ids)} 条结束任务记录；运行和排队任务已保留")

    def _stop_selected_job(self):
        job = self._selected_job()
        if not job:
            messagebox.showwarning("未选择任务", "请先在任务队列中选择要停止的产品")
            return
        status = str(job.get("status") or "")
        offer_id = str((job.get("inputs") or {}).get("offer_id") or "")
        if status == "completed":
            messagebox.showinfo("任务已完成", "该任务已经完成，无法通过停止按钮撤回 Ozon 商品")
            return
        if status == "cancelled":
            messagebox.showinfo("任务已停止", "该任务已经停止；可点击“修复选中任务”修改后继续")
            return
        if status == "stopping":
            messagebox.showinfo("正在停止", "停止请求已经发送，将在当前步骤结束后生效")
            return
        task_id = int(job.get("task_id") or 0)
        warning = (
            f"\n\n注意：Ozon 导入任务 {task_id} 已经提交，停止只能阻止后续查询和库存写入，"
            "不能撤回 Ozon 已接收的商品。"
            if task_id else
            "\n\n尚未提交到 Ozon 时，停止后可打开“修复选中任务”修改信息再继续。"
        )
        if not messagebox.askyesno(
            "只停止选中任务",
            f"确定停止选中的产品任务吗？\n货号：{offer_id}\n其他排队任务会继续运行。{warning}",
        ):
            return
        with self.auto_job_lock:
            status = str(job.get("status") or "")
            if status == "completed":
                messagebox.showinfo("任务刚刚完成", "该任务已在确认期间完成，无法通过停止按钮撤回")
                return
            if status == "queued":
                job["status"] = "cancelled"
                job["cancel_requested"] = False
                job["message"] = "已在运行前停止；其他排队任务不受影响"
                self.auto_status_var.set(f"已停止选中任务 {offer_id}；其他排队任务继续")
            elif status in {"running", "stopping"}:
                job["status"] = "stopping"
                job["cancel_requested"] = True
                job["message"] = "已收到停止请求；当前步骤结束后停止"
                self.auto_status_var.set(f"正在停止选中任务 {offer_id}；等待当前步骤结束")
            else:
                job["status"] = "cancelled"
                job["cancel_requested"] = False
                job["message"] = "任务已停止；其他排队任务不受影响"
            job["error"] = ""
            job["updated_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
            self._save_auto_jobs()
        self._refresh_job_tree()

    def _retry_selected_job(self):
        job = self._selected_job()
        if not job:
            messagebox.showwarning("未选择任务", "请先在任务队列中选择一个产品")
            return
        if job.get("status") in {"running", "queued", "stopping"}:
            messagebox.showwarning("任务已在队列中", "当前任务正在运行、等待运行或正在停止，不能重复加入队列")
            return
        if job.get("status") == "completed":
            messagebox.showinfo("任务已完成", "该任务已经完成，无需继续")
            return
        job["status"] = "queued"
        job["cancel_requested"] = False
        job["error"] = ""
        job["message"] = "等待继续"
        self._save_auto_jobs()
        self.auto_job_queue.put(job["id"])
        self._refresh_job_tree()
        self._start_auto_job_worker()

    def _open_job_repair(self):
        job = self._selected_job()
        if not job:
            messagebox.showwarning("未选择任务", "请先选择需要修复的产品")
            return
        if job.get("status") in {"running", "stopping"}:
            messagebox.showwarning("任务运行中", "请等待当前步骤停止后再修复")
            return
        self._build_job_repair_window(job)

    def _build_job_repair_window(self, job: dict):
        window = tk.Toplevel(self.root)
        window.title("修复产品任务 - " + str((job.get("inputs") or {}).get("offer_id") or ""))
        window.geometry("1120x860")
        window.minsize(980, 720)
        window.transient(self.root)
        container = ttk.Frame(window, padding=12)
        container.pack(fill=tk.BOTH, expand=True)
        container.columnconfigure(1, weight=1)
        container.columnconfigure(3, weight=1)
        ttk.Label(container, text="错误信息", font=("Microsoft YaHei UI", 11, "bold")).grid(row=0, column=0, sticky="nw", padx=5, pady=4)
        error_text = tk.Text(container, height=4, wrap="word", background="#fff4f2")
        error_text.grid(row=0, column=1, columnspan=3, sticky="ew", padx=5, pady=4)
        error_text.insert("1.0", str(job.get("error") or "没有记录错误"))
        error_text.configure(state=tk.DISABLED)
        inputs = dict(job.get("inputs") or {})
        inputs.setdefault("listing_mode", "follow")
        inputs.setdefault("local_image_paths", [])
        inputs.setdefault("advertising_percent", "15")
        inputs.setdefault("cargo_loss_percent", "10")
        inputs.setdefault("sales_commission_percent", "")
        repair_vars = {key: tk.StringVar(value=str(inputs.get(key) or "")) for key in JOB_INPUT_KEYS}
        repair_local_image_paths = [
            str(path) for path in (inputs.get("local_image_paths") or []) if str(path).strip()
        ]
        repair_local_images_changed = [False]
        ttk.Label(container, text="上品模式").grid(row=1, column=0, sticky="w", padx=5, pady=4)
        repair_mode_frame = ttk.Frame(container)
        repair_mode_frame.grid(row=1, column=1, columnspan=3, sticky="w", padx=5, pady=4)
        ttk.Radiobutton(
            repair_mode_frame, text="跟卖模式", variable=repair_vars["listing_mode"],
            value="follow",
        ).pack(side=tk.LEFT, padx=(0, 10))
        ttk.Radiobutton(
            repair_mode_frame, text="本地新品模式", variable=repair_vars["listing_mode"],
            value="local",
        ).pack(side=tk.LEFT)
        fields = (
            ("Ozon 链接/ID", "source_url"), ("货号", "offer_id"),
            ("1688 采购链接（可选）", "supplier_1688_url"),
            ("型号名称（同名合并多变体）", "model_name"), ("采购成本", "purchase_cost"),
            ("贴单费", "label_fee"), ("目标 ROI（%）", "target_roi"),
            ("标准佣金（%）", "sales_commission_percent"),
            ("广告费率（%）", "advertising_percent"), ("货损率（%）", "cargo_loss_percent"),
            ("自动售价", "price"), ("折扣前价格", "old_price"),
            ("商品净重(g)", "net_weight"),
            ("包装毛重(g)", "weight"), ("包装长(mm)", "depth"),
            ("包装宽(mm)", "width"), ("包装高(mm)", "height"),
        )
        row = 2
        repair_entries = {}
        for index in range(0, len(fields), 2):
            for offset, (label, key) in enumerate(fields[index:index + 2]):
                column = offset * 2
                ttk.Label(container, text=label).grid(row=row, column=column, sticky="w", padx=5, pady=4)
                entry = ttk.Entry(container, textvariable=repair_vars[key])
                entry.grid(row=row, column=column + 1, sticky="ew", padx=5, pady=4)
                repair_entries[key] = entry
            row += 1
        repair_normal_label_fee = [repair_vars["label_fee"].get().strip() or "2"]

        def update_repair_fbp_state():
            enabled = self._fbp_pricing_enabled({"fbp_pricing": repair_vars["fbp_pricing"].get()})
            if enabled:
                current = repair_vars["label_fee"].get().strip()
                if current and current != "0":
                    repair_normal_label_fee[0] = current
                repair_vars["label_fee"].set("0")
                repair_entries["label_fee"].state(["disabled"])
            else:
                repair_entries["label_fee"].state(["!disabled"])
                if repair_vars["label_fee"].get().strip() in {"", "0"}:
                    repair_vars["label_fee"].set(repair_normal_label_fee[0] or "2")

        ttk.Checkbutton(
            container, text="FBP 核价", variable=repair_vars["fbp_pricing"],
            onvalue="1", offvalue="0", command=update_repair_fbp_state,
        ).grid(row=row, column=0, columnspan=2, sticky="w", padx=5, pady=4)
        update_repair_fbp_state()
        row += 1
        ttk.Label(container, text="本地成品图片").grid(row=row, column=0, sticky="w", padx=5, pady=4)
        repair_local_images_frame = ttk.Frame(container)
        repair_local_images_frame.grid(row=row, column=1, columnspan=3, sticky="ew", padx=5, pady=4)
        repair_local_images_status = tk.StringVar()

        def refresh_repair_local_image_status():
            local_mode = self._local_listing_enabled({
                "listing_mode": repair_vars["listing_mode"].get(),
            })
            if local_mode:
                repair_local_images_status.set(
                    f"已选择 {len(repair_local_image_paths)} 张；第 1 张为主图"
                    if repair_local_image_paths else "尚未选择图片"
                )
            else:
                repair_local_images_status.set("跟卖模式无需选择")
            repair_local_images_button.state(["!disabled"] if local_mode else ["disabled"])
            repair_entries["source_url"].state(["disabled"] if local_mode else ["!disabled"])
            repair_entries["offer_id"].state(["readonly"] if local_mode else ["!readonly"])

        def select_repair_local_images():
            selected = filedialog.askopenfilenames(
                parent=window,
                title="替换本地新品图片（第 1 张为主图）",
                filetypes=[("图片", "*.jpg *.jpeg *.png *.webp")],
            )
            if selected:
                repair_local_image_paths[:] = [str(path) for path in selected[:15]]
                repair_local_images_changed[0] = True
                refresh_repair_local_image_status()

        repair_local_images_button = ttk.Button(
            repair_local_images_frame, text="重新选择图片", command=select_repair_local_images,
        )
        repair_local_images_button.pack(side=tk.LEFT)
        ttk.Label(
            repair_local_images_frame, textvariable=repair_local_images_status,
            foreground="#245a8d",
        ).pack(side=tk.LEFT, padx=(8, 0))
        for child in repair_mode_frame.winfo_children():
            child.configure(command=refresh_repair_local_image_status)
        refresh_repair_local_image_status()
        row += 1
        repair_shop_id = repair_vars["ozon_shop_id"].get().strip()
        shop_display_by_id = getattr(self, "ozon_shop_display_by_id", {})
        shop_id_by_display = getattr(self, "ozon_shop_id_by_display", {})
        repair_shop_display_var = tk.StringVar(value=shop_display_by_id.get(
            repair_shop_id,
            f"{repair_vars['ozon_shop_name'].get() or '原店铺'} | 配置已删除" if repair_shop_id else "",
        ))
        ttk.Label(container, text="上品店铺").grid(row=row, column=0, sticky="w", padx=5, pady=4)
        repair_shop_combo = ttk.Combobox(
            container, textvariable=repair_shop_display_var,
            values=list(shop_id_by_display), state="readonly", width=80,
        )
        repair_shop_combo.grid(row=row, column=1, columnspan=3, sticky="ew", padx=5, pady=4)
        row += 1
        ttk.Label(container, text="人工类目").grid(row=row, column=0, sticky="w", padx=5, pady=4)
        repair_category = ttk.Combobox(container, textvariable=repair_vars["category_display"], values=self.category_displays, width=80)
        repair_category.grid(row=row, column=1, columnspan=2, sticky="ew", padx=5, pady=4)
        repair_category.bind("<KeyRelease>", self._filter_category_choices)
        repair_load_attributes_button = ttk.Button(container, text="加载/刷新当前类目属性")
        repair_load_attributes_button.grid(row=row, column=3, sticky="ew", padx=5, pady=4)
        row += 1

        state = job.get("state") if isinstance(job.get("state"), dict) else {}
        ttk.Label(container, text="标题/标签/属性（用于修复 Ozon 校验错误）").grid(row=row, column=0, columnspan=4, sticky="w", padx=5, pady=(10, 3))
        row += 1
        content_tabs = ttk.Notebook(container)
        content_tabs.grid(row=row, column=0, columnspan=4, sticky="nsew", padx=5)
        container.rowconfigure(row, weight=1)
        basic = ttk.Frame(content_tabs, padding=8)
        attribute_repair = ttk.Frame(content_tabs, padding=8)
        attrs_frame = ttk.Frame(content_tabs, padding=8)
        complex_frame = ttk.Frame(content_tabs, padding=8)
        content_tabs.add(basic, text="文案")
        content_tabs.add(attribute_repair, text="商品属性修复")
        content_tabs.add(attrs_frame, text="高级普通属性 JSON")
        content_tabs.add(complex_frame, text="高级组合属性 JSON")
        basic.columnconfigure(1, weight=1)
        state_fields = state.get("fields") if isinstance(state.get("fields"), dict) else {}
        title_var = tk.StringVar(value=str(state_fields.get("title") or ""))
        tags_var = tk.StringVar(value=format_ozon_hashtags(state_fields.get("tags") or ""))
        ttk.Label(basic, text="俄文标题").grid(row=0, column=0, sticky="w", pady=4)
        ttk.Entry(basic, textvariable=title_var).grid(row=0, column=1, sticky="ew", pady=4)
        ttk.Label(basic, text="主题标签").grid(row=1, column=0, sticky="w", pady=4)
        ttk.Entry(basic, textvariable=tags_var).grid(row=1, column=1, sticky="ew", pady=4)
        ttk.Label(basic, text="俄文描述").grid(row=2, column=0, sticky="nw", pady=4)
        description_text = tk.Text(basic, height=12, wrap="word")
        description_text.grid(row=2, column=1, sticky="nsew", pady=4)
        description_text.insert("1.0", str(state.get("description") or ""))
        basic.rowconfigure(2, weight=1)
        attrs_text = tk.Text(attrs_frame, wrap="none", font=("Consolas", 10))
        attrs_text.pack(fill=tk.BOTH, expand=True)
        attrs_text.insert("1.0", str(state.get("attributes_json") or "[]"))
        complex_text = tk.Text(complex_frame, wrap="none", font=("Consolas", 10))
        complex_text.pack(fill=tk.BOTH, expand=True)
        complex_text.insert("1.0", str(state.get("complex_attributes_json") or "[]"))

        repair_definitions = [
            dict(item) for item in (state.get("attribute_definitions") or [])
            if isinstance(item, dict) and item.get("id")
        ]
        repair_definition_by_id = {
            int(item["id"]): item for item in repair_definitions
        }
        repair_dictionary_state = {
            "attribute_id": 0,
            "all": [],
            "visible": {},
        }
        repair_error_ids = {
            int(value) for value in re.findall(
                r"""attribute[_\s-]?id["']?\s*[:=]\s*["']?(\d+)""",
                str(job.get("error") or ""),
                flags=re.I,
            )
        }
        try:
            initial_definition_key = (
                int(inputs.get("category_id") or 0),
                int(inputs.get("type_id") or 0),
            )
        except (TypeError, ValueError):
            initial_definition_key = (0, 0)
        loaded_definition_key = [initial_definition_key]

        attribute_repair.columnconfigure(0, weight=1)
        attribute_repair.rowconfigure(1, weight=1)
        ttk.Label(
            attribute_repair,
            text=(
                "选中属性后：自由文本属性在下方直接修改；字典/下拉属性必须加载 Ozon 选项后选择。"
                "红色行是本次错误涉及的属性或缺失的必填属性。"
            ),
            foreground="#245a8d",
            wraplength=1000,
        ).grid(row=0, column=0, sticky="ew", pady=(0, 6))
        repair_tree_frame = ttk.Frame(attribute_repair)
        repair_tree_frame.grid(row=1, column=0, sticky="nsew")
        repair_tree_frame.columnconfigure(0, weight=1)
        repair_tree_frame.rowconfigure(0, weight=1)
        repair_attribute_tree = ttk.Treeview(
            repair_tree_frame,
            columns=("required", "kind", "value", "problem"),
            show="tree headings",
            selectmode="browse",
            height=10,
        )
        repair_attribute_tree.grid(row=0, column=0, sticky="nsew")
        repair_scroll = ttk.Scrollbar(
            repair_tree_frame, orient=tk.VERTICAL, command=repair_attribute_tree.yview,
        )
        repair_scroll.grid(row=0, column=1, sticky="ns")
        repair_attribute_tree.configure(yscrollcommand=repair_scroll.set)
        repair_attribute_tree.heading("#0", text="当前类目属性")
        repair_attribute_tree.heading("required", text="必填")
        repair_attribute_tree.heading("kind", text="填写方式")
        repair_attribute_tree.heading("value", text="当前值")
        repair_attribute_tree.heading("problem", text="状态/依据")
        repair_attribute_tree.column("#0", width=245, stretch=True)
        repair_attribute_tree.column("required", width=50, anchor="center", stretch=False)
        repair_attribute_tree.column("kind", width=105, anchor="center", stretch=False)
        repair_attribute_tree.column("value", width=330, stretch=True)
        repair_attribute_tree.column("problem", width=260, stretch=True)
        repair_attribute_tree.tag_configure("problem", foreground="#b3261e")

        repair_editor = ttk.LabelFrame(attribute_repair, text="编辑选中属性", padding=8)
        repair_editor.grid(row=2, column=0, sticky="ew", pady=(8, 0))
        repair_editor.columnconfigure(1, weight=1)
        repair_editor.columnconfigure(2, weight=1)
        repair_editor.columnconfigure(3, weight=1)
        repair_selected_label = ttk.Label(
            repair_editor, text="请先在上表选择需要修复的属性", wraplength=980,
        )
        repair_selected_label.grid(row=0, column=0, columnspan=6, sticky="w", pady=(0, 6))
        repair_text_value_var = tk.StringVar()
        ttk.Label(repair_editor, text="自由文本值").grid(row=1, column=0, sticky="w", padx=(0, 5))
        repair_text_entry = ttk.Entry(repair_editor, textvariable=repair_text_value_var)
        repair_text_entry.grid(row=1, column=1, columnspan=3, sticky="ew", padx=5)
        repair_save_text_button = ttk.Button(repair_editor, text="保存文本值")
        repair_save_text_button.grid(row=1, column=4, sticky="ew", padx=4)
        repair_clear_button = ttk.Button(repair_editor, text="清空该属性")
        repair_clear_button.grid(row=1, column=5, sticky="ew", padx=4)

        repair_dictionary_query_var = tk.StringVar()
        ttk.Label(repair_editor, text="字典选项筛选").grid(row=2, column=0, sticky="w", padx=(0, 5), pady=(7, 0))
        repair_dictionary_query = ttk.Entry(
            repair_editor, textvariable=repair_dictionary_query_var,
        )
        repair_dictionary_query.grid(row=2, column=1, columnspan=2, sticky="ew", padx=5, pady=(7, 0))
        repair_load_dictionary_button = ttk.Button(repair_editor, text="加载全部选项")
        repair_load_dictionary_button.grid(row=2, column=3, sticky="ew", padx=4, pady=(7, 0))
        repair_refresh_dictionary_button = ttk.Button(repair_editor, text="刷新选项缓存")
        repair_refresh_dictionary_button.grid(row=2, column=4, sticky="ew", padx=4, pady=(7, 0))
        repair_dictionary_combo = ttk.Combobox(repair_editor, state="readonly")
        repair_dictionary_combo.grid(row=3, column=0, columnspan=5, sticky="ew", pady=(7, 0), padx=(0, 4))
        repair_apply_dictionary_button = ttk.Button(repair_editor, text="采用选中值")
        repair_apply_dictionary_button.grid(row=3, column=5, sticky="ew", padx=4, pady=(7, 0))
        repair_save_text_button.state(["disabled"])
        repair_load_dictionary_button.state(["disabled"])
        repair_refresh_dictionary_button.state(["disabled"])
        repair_apply_dictionary_button.state(["disabled"])
        repair_attribute_status = ttk.Label(
            attribute_repair,
            text="正在准备属性修复页……",
            foreground="#666",
            wraplength=1000,
        )
        repair_attribute_status.grid(row=3, column=0, sticky="ew", pady=(6, 0))

        def read_repair_documents():
            return (
                parse_attributes(attrs_text.get("1.0", tk.END)),
                parse_complex_attributes(complex_text.get("1.0", tk.END)),
            )

        def write_repair_documents(attributes, complex_attributes):
            attrs_text.delete("1.0", tk.END)
            attrs_text.insert("1.0", json.dumps(attributes, ensure_ascii=False, indent=2))
            complex_text.delete("1.0", tk.END)
            complex_text.insert("1.0", json.dumps(complex_attributes, ensure_ascii=False, indent=2))

        def remove_repair_attribute(attributes, complex_attributes, attribute_id):
            attributes[:] = [
                item for item in attributes
                if int(item.get("id") or 0) != attribute_id
            ]
            for group in complex_attributes:
                group["attributes"] = [
                    item for item in group.get("attributes", [])
                    if int(item.get("id") or 0) != attribute_id
                ]
            complex_attributes[:] = [
                group for group in complex_attributes if group.get("attributes")
            ]

        def refresh_repair_attribute_tree(show_error=True):
            for item_id in repair_attribute_tree.get_children():
                repair_attribute_tree.delete(item_id)
            if not repair_definitions:
                repair_attribute_status.configure(
                    text="尚未读取到属性定义；请点击上方“加载/刷新当前类目属性”。",
                    foreground="#b3261e",
                )
                return
            try:
                attributes, complex_attributes = read_repair_documents()
            except Exception as error:
                repair_attribute_status.configure(
                    text="高级属性 JSON 格式错误：" + str(error),
                    foreground="#b3261e",
                )
                if show_error:
                    messagebox.showerror("属性 JSON 错误", str(error), parent=window)
                return
            current = attribute_values_by_id(attributes, complex_attributes)
            missing_ids = {
                int(definition["id"]) for definition in repair_definitions
                if definition.get("is_required")
                and not (
                    current.get(int(definition["id"]))
                    and any(
                        value.get("value") or value.get("dictionary_value_id")
                        for value in current[int(definition["id"])].get("values", [])
                    )
                )
            }
            sorted_definitions = sorted(
                repair_definitions,
                key=lambda item: (
                    int(item["id"]) not in repair_error_ids
                    and int(item["id"]) not in missing_ids,
                    not bool(item.get("is_required")),
                    str(item.get("group_name") or ""),
                    str(item.get("name") or "").casefold(),
                ),
            )
            problem_item_ids = []
            for definition in sorted_definitions:
                attribute_id = int(definition["id"])
                item = current.get(attribute_id)
                value = self._display_attribute_value(item)
                name = str(definition.get("name") or definition.get("name_ru") or attribute_id)
                is_missing = attribute_id in missing_ids
                if attribute_id in repair_error_ids:
                    problem = "本次 Ozon 错误涉及此属性"
                elif is_missing:
                    problem = "必填属性缺失"
                else:
                    problem = str((item or {}).get("_source") or "")
                kind = "字典下拉" if int(definition.get("dictionary_id") or 0) else "自由文本"
                if definition.get("is_collection"):
                    kind += " / 可多选"
                tags = ("problem",) if attribute_id in repair_error_ids or is_missing else ()
                if tags:
                    problem_item_ids.append(str(attribute_id))
                repair_attribute_tree.insert(
                    "", tk.END, iid=str(attribute_id), text=name,
                    values=(
                        "是" if definition.get("is_required") else "",
                        kind,
                        value,
                        problem,
                    ),
                    tags=tags,
                )
            if problem_item_ids and not repair_attribute_tree.selection():
                target_id = problem_item_ids[0]
                repair_attribute_tree.selection_set(target_id)
                repair_attribute_tree.focus(target_id)
                repair_attribute_tree.see(target_id)
                repair_attribute_selected()
            repair_attribute_status.configure(
                text=(
                    f"当前类目共 {len(repair_definitions)} 项属性，仍缺必填 {len(missing_ids)} 项。"
                    "修改后会同步到高级 JSON，并只继续当前选中任务。"
                ),
                foreground="#666" if not missing_ids else "#b3261e",
            )

        def selected_repair_definition():
            selected = repair_attribute_tree.selection()
            if len(selected) != 1:
                return None
            try:
                return repair_definition_by_id.get(int(selected[0]))
            except (TypeError, ValueError):
                return None

        def filter_repair_dictionary_choices(_event=None):
            query = repair_dictionary_query_var.get().strip().casefold().replace("ё", "е")
            visible = []
            for option in repair_dictionary_state["all"]:
                display = self._dictionary_option_display(option)
                search_text = " ".join((
                    display,
                    str(option.get("value") or ""),
                    str(option.get("info") or ""),
                )).casefold().replace("ё", "е")
                if not query or query in search_text:
                    visible.append(option)
            mapping = {
                self._dictionary_option_display(option): option for option in visible
            }
            repair_dictionary_state["visible"] = mapping
            repair_dictionary_combo.configure(values=list(mapping))
            repair_dictionary_combo.set(next(iter(mapping), ""))
            repair_attribute_status.configure(
                text=(
                    f"字典缓存共 {len(repair_dictionary_state['all'])} 项；"
                    f"当前匹配 {len(mapping)} 项。只能采用列表中的 Ozon 规定值。"
                ),
                foreground="#666",
            )

        def install_repair_dictionary_values(attribute_id, results, source_label):
            if not window.winfo_exists():
                return
            definition = selected_repair_definition()
            if not definition or int(definition["id"]) != int(attribute_id):
                return
            repair_dictionary_state["attribute_id"] = int(attribute_id)
            repair_dictionary_state["all"] = [
                item for item in results
                if isinstance(item, dict) and item.get("id")
            ]
            repair_dictionary_query_var.set(repair_text_value_var.get().strip())
            filter_repair_dictionary_choices()
            repair_attribute_status.configure(
                text=(
                    f"已从{source_label}读取 {len(repair_dictionary_state['all'])} 个 Ozon 可选值；"
                    "请选择后点击“采用选中值”。"
                ),
                foreground="#245a8d",
            )
            repair_load_dictionary_button.state(["!disabled"])
            repair_refresh_dictionary_button.state(["!disabled"])

        def load_repair_dictionary_values(force_refresh=False):
            definition = selected_repair_definition()
            if not definition:
                messagebox.showwarning("未选择属性", "请先在属性表中选择一项", parent=window)
                return
            if not int(definition.get("dictionary_id") or 0):
                messagebox.showwarning("不是字典属性", "该属性应直接填写自由文本", parent=window)
                return
            attribute_id = int(definition["id"])
            if self._is_brand_attribute(definition):
                install_repair_dictionary_values(
                    attribute_id, [self._locked_no_brand_dictionary_value()], "系统锁定值",
                )
                return
            try:
                category_id = int(repair_vars["category_id"].get())
                type_id = int(repair_vars["type_id"].get())
            except (TypeError, ValueError):
                messagebox.showerror("类目无效", "请先选择有效类目并读取属性", parent=window)
                return
            repair_load_dictionary_button.state(["disabled"])
            repair_refresh_dictionary_button.state(["disabled"])
            repair_attribute_status.configure(text="正在读取该属性的全部 Ozon 字典选项……", foreground="#666")

            def worker():
                try:
                    api = self._api()
                    results, from_cache, _path = self._retry_step(
                        "修复窗口字典读取",
                        lambda: self._cached_dictionary_values(
                            api, category_id, type_id, attribute_id,
                            force_refresh=force_refresh,
                        ),
                        attempts=3,
                    )
                    source_label = "本地缓存" if from_cache else "Ozon 实时字典"
                    self.root.after(
                        0,
                        lambda: install_repair_dictionary_values(
                            attribute_id, results, source_label,
                        ),
                    )
                except Exception as error:
                    error_message = str(error) or type(error).__name__

                    def show_dictionary_error(message=error_message):
                        if not window.winfo_exists():
                            return
                        repair_load_dictionary_button.state(["!disabled"])
                        repair_refresh_dictionary_button.state(["!disabled"])
                        repair_attribute_status.configure(
                            text="字典读取失败：" + message, foreground="#b3261e",
                        )
                        messagebox.showerror("字典读取失败", message, parent=window)

                    self.root.after(0, show_dictionary_error)

            threading.Thread(target=worker, daemon=True).start()

        def repair_attribute_selected(_event=None):
            definition = selected_repair_definition()
            if not definition:
                return
            try:
                attributes, complex_attributes = read_repair_documents()
                item = attribute_values_by_id(attributes, complex_attributes).get(int(definition["id"]))
            except Exception:
                item = None
            current_value = ""
            if item and item.get("values"):
                current_value = str(item["values"][0].get("value") or "")
            repair_text_value_var.set(current_value)
            repair_dictionary_query_var.set(current_value)
            repair_dictionary_state.update({"attribute_id": 0, "all": [], "visible": {}})
            repair_dictionary_combo.configure(values=[])
            repair_dictionary_combo.set("")
            required = "必填" if definition.get("is_required") else "选填"
            description = re.sub(
                r"\s+", " ", str(definition.get("description") or "")
            ).strip()
            repair_selected_label.configure(
                text=(
                    f"{definition.get('name') or definition.get('name_ru')} "
                    f"（ID {definition.get('id')}，{required}）"
                    + (f"；{description}" if description else "")
                )
            )
            is_dictionary = bool(int(definition.get("dictionary_id") or 0))
            repair_save_text_button.state(["disabled"] if is_dictionary else ["!disabled"])
            repair_load_dictionary_button.state(["!disabled"] if is_dictionary else ["disabled"])
            repair_refresh_dictionary_button.state(["!disabled"] if is_dictionary else ["disabled"])
            repair_apply_dictionary_button.state(["!disabled"] if is_dictionary else ["disabled"])
            repair_attribute_status.configure(
                text=(
                    "这是 Ozon 字典/下拉属性：请加载选项并从列表选择，不能保存 AI 自由文本。"
                    if is_dictionary else
                    "这是自由文本属性：请填写俄文、数字或 Ozon 允许的格式后保存。"
                ),
                foreground="#245a8d",
            )

        def save_repair_text_value():
            definition = selected_repair_definition()
            if not definition:
                messagebox.showwarning("未选择属性", "请先选择需要修复的属性", parent=window)
                return
            if int(definition.get("dictionary_id") or 0):
                messagebox.showwarning(
                    "必须选择规定值",
                    "该属性是 Ozon 字典/下拉属性，不能输入自由文本。",
                    parent=window,
                )
                return
            value = repair_text_value_var.get().strip()
            if not value:
                messagebox.showwarning("属性值为空", "请输入属性值，或使用“清空该属性”", parent=window)
                return
            try:
                attributes, complex_attributes = read_repair_documents()
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=value, source="任务修复窗口：人工填写",
                )
                write_repair_documents(attributes, complex_attributes)
                refresh_repair_attribute_tree(show_error=False)
            except Exception as error:
                messagebox.showerror("属性保存失败", str(error), parent=window)

        def apply_repair_dictionary_value():
            definition = selected_repair_definition()
            selected = repair_dictionary_state["visible"].get(
                repair_dictionary_combo.get(),
            )
            if (
                not definition
                or int(definition["id"]) != int(repair_dictionary_state["attribute_id"] or 0)
                or not selected
            ):
                messagebox.showwarning(
                    "未选择有效选项",
                    "请先加载当前属性的全部选项，并从下拉列表中选择一个 Ozon 规定值。",
                    parent=window,
                )
                return
            if self._is_brand_attribute(definition):
                selected = self._locked_no_brand_dictionary_value()
            try:
                attributes, complex_attributes = read_repair_documents()
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=str(selected.get("value") or ""),
                    dictionary_value_id=int(selected["id"]),
                    source="任务修复窗口：Ozon 字典人工选择",
                    append=bool(definition.get("is_collection")),
                )
                write_repair_documents(attributes, complex_attributes)
                repair_text_value_var.set(str(selected.get("value") or ""))
                refresh_repair_attribute_tree(show_error=False)
            except Exception as error:
                messagebox.showerror("字典值保存失败", str(error), parent=window)

        def clear_repair_attribute():
            definition = selected_repair_definition()
            if not definition:
                messagebox.showwarning("未选择属性", "请先选择需要清空的属性", parent=window)
                return
            try:
                attributes, complex_attributes = read_repair_documents()
                remove_repair_attribute(
                    attributes, complex_attributes, int(definition["id"]),
                )
                write_repair_documents(attributes, complex_attributes)
                repair_text_value_var.set("")
                repair_dictionary_query_var.set("")
                refresh_repair_attribute_tree(show_error=False)
            except Exception as error:
                messagebox.showerror("属性清空失败", str(error), parent=window)

        def load_repair_definitions():
            display = repair_vars["category_display"].get().strip()
            category = self.category_by_display.get(display)
            if category:
                repair_vars["category_id"].set(str(category["description_category_id"]))
                repair_vars["type_id"].set(str(category["type_id"]))
            try:
                category_id = int(repair_vars["category_id"].get())
                type_id = int(repair_vars["type_id"].get())
            except (TypeError, ValueError):
                messagebox.showerror(
                    "类目无效",
                    "请从类目下拉列表选择准确类目后再读取属性。",
                    parent=window,
                )
                return
            repair_load_attributes_button.state(["disabled"])
            repair_attribute_status.configure(
                text="正在读取当前类目的中俄双语实时属性……",
                foreground="#666",
            )

            def worker():
                try:
                    definitions = self._retry_step(
                        "修复窗口类目属性读取",
                        lambda: self._localized_attributes(self._api(), category_id, type_id),
                        attempts=3,
                    )
                    if not definitions:
                        raise RuntimeError("Ozon 没有返回该类目的属性")

                    def install_definitions():
                        if not window.winfo_exists():
                            return
                        if loaded_definition_key[0] != (category_id, type_id):
                            write_repair_documents([], [])
                        loaded_definition_key[0] = (category_id, type_id)
                        repair_definitions[:] = [
                            dict(item) for item in definitions
                            if isinstance(item, dict) and item.get("id")
                        ]
                        repair_definition_by_id.clear()
                        repair_definition_by_id.update({
                            int(item["id"]): item for item in repair_definitions
                        })
                        repair_load_attributes_button.state(["!disabled"])
                        refresh_repair_attribute_tree(show_error=False)

                    self.root.after(0, install_definitions)
                except Exception as error:
                    error_message = str(error) or type(error).__name__

                    def show_definition_error(message=error_message):
                        if not window.winfo_exists():
                            return
                        repair_load_attributes_button.state(["!disabled"])
                        repair_attribute_status.configure(
                            text="类目属性读取失败：" + message,
                            foreground="#b3261e",
                        )
                        messagebox.showerror("类目属性读取失败", message, parent=window)

                    self.root.after(0, show_definition_error)

            threading.Thread(target=worker, daemon=True).start()

        repair_attribute_tree.bind("<<TreeviewSelect>>", repair_attribute_selected)
        repair_dictionary_query.bind("<KeyRelease>", filter_repair_dictionary_choices)
        repair_save_text_button.configure(command=save_repair_text_value)
        repair_clear_button.configure(command=clear_repair_attribute)
        repair_load_dictionary_button.configure(
            command=lambda: load_repair_dictionary_values(False),
        )
        repair_refresh_dictionary_button.configure(
            command=lambda: load_repair_dictionary_values(True),
        )
        repair_apply_dictionary_button.configure(command=apply_repair_dictionary_value)
        repair_load_attributes_button.configure(command=load_repair_definitions)
        repair_category.bind(
            "<<ComboboxSelected>>", lambda _event: load_repair_definitions(),
        )
        repair_category.bind("<Return>", lambda _event: load_repair_definitions())
        ttk.Button(
            attrs_frame,
            text="将高级 JSON 重新应用到属性修复表",
            command=lambda: refresh_repair_attribute_tree(show_error=True),
        ).pack(side=tk.BOTTOM, anchor="e", pady=(5, 0))
        ttk.Button(
            complex_frame,
            text="将高级 JSON 重新应用到属性修复表",
            command=lambda: refresh_repair_attribute_tree(show_error=True),
        ).pack(side=tk.BOTTOM, anchor="e", pady=(5, 0))
        content_tabs.select(attribute_repair)
        refresh_repair_attribute_tree(show_error=False)
        if not repair_definitions:
            window.after(100, load_repair_definitions)

        def save_and_continue():
            updated_inputs = {key: variable.get().strip() for key, variable in repair_vars.items()}
            if self._local_listing_enabled(updated_inputs):
                updated_inputs["source_url"] = ""
                updated_inputs["local_image_paths"] = list(repair_local_image_paths)
            else:
                updated_inputs["local_image_paths"] = []
            selected_repair_shop_id = shop_id_by_display.get(repair_shop_display_var.get().strip(), "")
            if selected_repair_shop_id:
                updated_inputs["ozon_shop_id"] = selected_repair_shop_id
                updated_inputs["ozon_shop_name"] = self.ozon_shops[selected_repair_shop_id]["name"]
            category = self.category_by_display.get(updated_inputs.get("category_display", ""))
            if category:
                updated_inputs["category_id"] = str(category["description_category_id"])
                updated_inputs["type_id"] = str(category["type_id"])
            try:
                self._validate_job_inputs(updated_inputs)
                if int(job.get("stage") or 0) >= 2 or tags_var.get().strip():
                    validate_ozon_hashtags(tags_var.get())
                repaired_attributes = parse_attributes(attrs_text.get("1.0", tk.END))
                repaired_complex_attributes = parse_complex_attributes(
                    complex_text.get("1.0", tk.END),
                )
                if int(job.get("stage") or 0) >= 5 and repair_definitions:
                    missing = validate_required(
                        repaired_attributes, repair_definitions,
                        repaired_complex_attributes,
                    )
                    if missing:
                        content_tabs.select(attribute_repair)
                        raise ValueError(
                            "仍缺少以下 Ozon 必填属性，请先在“商品属性修复”页填写："
                            + "、".join(missing)
                        )
            except Exception as error:
                messagebox.showerror("修复内容仍有错误", str(error), parent=window)
                return
            old_inputs = dict(job.get("inputs") or {})
            stage = int(job.get("stage") or 0)
            old_state = dict(job.get("state") or {})
            old_state_fields = dict(old_state.get("fields") or {})
            definitions_changed = repair_definitions != [
                item for item in (old_state.get("attribute_definitions") or [])
                if isinstance(item, dict) and item.get("id")
            ]
            content_changed = any((
                title_var.get().strip() != str(old_state_fields.get("title") or "").strip(),
                format_ozon_hashtags(tags_var.get()) != format_ozon_hashtags(old_state_fields.get("tags") or ""),
                description_text.get("1.0", tk.END).strip() != str(old_state.get("description") or "").strip(),
                attrs_text.get("1.0", tk.END).strip() != str(old_state.get("attributes_json") or "[]").strip(),
                complex_text.get("1.0", tk.END).strip() != str(old_state.get("complex_attributes_json") or "[]").strip(),
                definitions_changed,
            ))
            changed_input_keys = {
                key for key in JOB_INPUT_KEYS
                if str(updated_inputs.get(key) or "").strip()
                != str(old_inputs.get(key) or "").strip()
            }
            local_images_changed = (
                list(updated_inputs.get("local_image_paths") or [])
                != list(old_inputs.get("local_image_paths") or [])
            )
            submission_inputs_changed = bool(
                changed_input_keys - {"supplier_1688_url"}
            ) or local_images_changed
            if (
                updated_inputs.get("listing_mode") != old_inputs.get("listing_mode", "follow")
                or updated_inputs.get("source_url") != old_inputs.get("source_url")
                or local_images_changed
            ):
                stage = 0
                job["state"] = {}
            elif any(updated_inputs.get(key) != old_inputs.get(key) for key in ("category_display", "category_id", "type_id", "net_weight")):
                stage = min(stage, 4)
            elif changed_input_keys.intersection({"watermark_path", "image_prompt"}):
                stage = min(stage, 2)
            elif submission_inputs_changed or content_changed:
                stage = min(stage, 5)
            existing_task_id = int(job.get("task_id") or 0)
            import_failed = "商品导入失败" in str(job.get("error") or "")
            changed_after_ozon_submit = bool(
                existing_task_id and (submission_inputs_changed or content_changed)
            )
            if changed_after_ozon_submit and not import_failed:
                messagebox.showerror(
                    "Ozon 已接收原任务",
                    f"Ozon 导入任务 {existing_task_id} 已经提交，程序无法撤回。为避免把仍在处理的商品重复提交，"
                    "本任务不能直接修改后重发。\n\n请先到 Ozon 后台按任务 ID 或 Offer ID 处理；"
                    "如果确实要作为新商品重新上架，请新建任务并使用新的 Offer ID。",
                    parent=window,
                )
                return
            if self._local_listing_enabled(updated_inputs) and repair_local_images_changed[0]:
                try:
                    updated_inputs["local_image_paths"] = self._snapshot_local_job_images(
                        updated_inputs["local_image_paths"], updated_inputs["offer_id"],
                        str(job.get("id") or "repair"),
                    )
                except Exception as error:
                    messagebox.showerror("本地图片保存失败", str(error), parent=window)
                    return
            repaired_state = dict(job.get("state") or {})
            repaired_fields = dict(repaired_state.get("fields") or {})
            repaired_fields.update(updated_inputs)
            repaired_fields["title"] = title_var.get().strip()
            repaired_fields["tags"] = format_ozon_hashtags(tags_var.get())
            repaired_state["fields"] = repaired_fields
            repaired_state["description"] = description_text.get("1.0", tk.END).strip()
            repaired_state["attributes_json"] = attrs_text.get("1.0", tk.END).strip() or "[]"
            repaired_state["complex_attributes_json"] = complex_text.get("1.0", tk.END).strip() or "[]"
            repaired_state["attribute_definitions"] = list(repair_definitions)
            job["inputs"] = updated_inputs
            job["state"] = repaired_state
            job["stage"] = stage
            if updated_inputs.get("offer_id") != old_inputs.get("offer_id") or stage < 6:
                job["task_id"] = 0
                job["import_completed"] = False
                job["stock_completed"] = False
                job["ledger_completed"] = False
            elif "supplier_1688_url" in changed_input_keys:
                job["ledger_completed"] = False
            job["status"] = "queued"
            job["cancel_requested"] = False
            job["error"] = ""
            job["message"] = f"已修复，从阶段 {stage + 1} 继续"
            self._save_auto_jobs()
            self.auto_job_queue.put(job["id"])
            self._refresh_job_tree()
            window.destroy()
            self._start_auto_job_worker()

        buttons = ttk.Frame(container)
        buttons.grid(row=row + 1, column=0, columnspan=4, sticky="e", pady=(10, 0))
        ttk.Button(buttons, text="取消", command=window.destroy).pack(side=tk.RIGHT, padx=5)
        ttk.Button(buttons, text="保存修复并继续运行", command=save_and_continue).pack(side=tk.RIGHT, padx=5)
        return window

    def _validate_workbench_fields(self):
        self._category_selected()
        local_mode = self._active_listing_is_local()
        if not local_mode and not self.vars["source_url"].get().strip():
            raise ValueError("请填写 Ozon 链接或商品 ID")
        if local_mode and not (self.source_images or self.local_images):
            raise ValueError("本地新品模式请至少选择 1 张成品图片")
        if not self.vars["offer_id"].get().strip():
            raise ValueError("请填写上传货号（Offer ID）")
        try:
            price = Decimal(self.vars["price"].get().strip())
            old_price = Decimal(self.vars["old_price"].get().strip() or "0")
        except (InvalidOperation, ValueError) as error:
            raise ValueError("售价和折扣前价格必须是有效数字") from error
        if price <= 0:
            raise ValueError("售价必须大于 0")
        if old_price < 0 or (old_price and old_price <= price):
            raise ValueError("折扣前价格必须大于售价；不设置折扣时填写 0")
        physical_values = {}
        for key, label in (("net_weight", "商品重量"), ("weight", "毛重"), ("depth", "包装长度"), ("width", "包装宽度"), ("height", "包装高度")):
            try:
                number = float(self.vars[key].get())
            except ValueError as error:
                raise ValueError(label + "必须是有效数字") from error
            if number <= 0:
                raise ValueError(label + "必须大于 0")
            physical_values[key] = number
        if physical_values["weight"] < physical_values["net_weight"]:
            raise ValueError("毛重（含包装）不能小于商品净重")
        display = self.vars["category_display"].get().strip()
        category = self.category_by_display.get(display)
        if not category:
            raise ValueError("请先加载 Ozon 实时类目，并从下拉列表中人工选择一个类目")
        if not self.vars["warehouse_id"].get().strip():
            raise ValueError("请先在“草稿与上架”页面选择 RFBS 仓库")
        try:
            stock = int(self.vars["stock"].get())
        except ValueError as error:
            raise ValueError("可售库存必须是整数") from error
        if stock < 0:
            raise ValueError("可售库存不能小于 0")
        watermark = self.vars["watermark_path"].get().strip()
        if not local_mode and not Path(watermark).is_file():
            raise ValueError("请先在“商品与图片”页面选择有效的水印文件")
        if local_mode and watermark and not Path(watermark).is_file():
            raise ValueError("已填写的水印文件不存在；请重新选择或留空")
        return category

    def _confirm_auto_pipeline(self):
        if self.busy:
            messagebox.showwarning("请稍候", "已有任务正在运行")
            return
        try:
            category = self._validate_workbench_fields()
            self._save_workspace()
        except Exception as error:
            messagebox.showerror("录入内容不完整", str(error))
            return
        summary = (
            f"货号：{self.vars['offer_id'].get()}\n"
            f"售价 / 折扣前价格：{self.vars['price'].get()} / {self.vars['old_price'].get()} RUB\n"
            f"商品净重 / 毛重：{self.vars['net_weight'].get()} / {self.vars['weight'].get()} g\n"
            f"包装：{self.vars['depth'].get()} × {self.vars['width'].get()} × {self.vars['height'].get()} mm\n"
            f"类目：{category['display']}\n\n"
            "确认后程序会自动运行并真实创建/更新 Ozon 商品、写入 RFBS 库存。是否继续？"
        )
        if messagebox.askyesno("确认自动上架", summary):
            self._run(self._auto_pipeline)

    def _auto_pipeline(self):
        self.auto_pipeline_running = True
        try:
            local_mode = self._active_listing_is_local()
            if local_mode:
                self._set_auto_progress(1, "1/9 正在读取并分析本地新品图片")
                self._retry_step(
                    "本地图片视觉分析",
                    lambda: self._prepare_local_job_images(analyze=True), attempts=3,
                )
            else:
                self._set_auto_progress(1, "1/9 正在读取指定 Ozon 商品并下载高清原图")
                self._retry_step("商品解析/原图下载", self._parse_url, attempts=3)
            self._ui_call(lambda: None)

            self._set_auto_progress(2, "2/9 正在生成俄文标题、描述和合规标签")
            self._retry_step("文案生成", self._generate_copywriting, attempts=3)
            self._ui_call(lambda: None)

            if local_mode:
                self._set_auto_progress(3, "3/9 正在准备成品图片并按需添加水印")
                self._retry_step(
                    "本地成品图片处理", self._prepare_local_images_for_upload,
                    attempts=2, delay_seconds=2,
                )
            else:
                self._set_auto_progress(3, "3/9 正在分析全部原图、生成新主图并给全部图片加水印")
                self._retry_step("视觉分析/主图生成", self._generate_images, attempts=2, delay_seconds=4)
            self._ui_call(lambda: None)

            self._set_auto_progress(4, "4/9 正在上传处理后的图片到 OSS")
            self._retry_step("OSS 图片上传", self._upload_images, attempts=3)

            self._set_auto_progress(5, "5/9 正在读取手工指定类目的实时属性并由 AI 填写")
            self._retry_step(
                "类目属性填写", lambda: self._ai_fill_category_attributes(use_selected_category=True),
                attempts=3, delay_seconds=3,
            )
            self._ui_call(lambda: None)

            self._set_auto_progress(6, "6/9 正在校验数据并保存本地草稿")
            draft_path = self._save_draft(notify=False)
            self._ui_call(self._save_workspace_quietly)

            self._set_auto_progress(7, "7/9 正在提交 Ozon；商品创建请求不会盲目重试")
            submitted_path = self._submit(notify=False, auto_progress=True)

            self._set_auto_progress(9, "9/9 自动上架完成，已提交商品并写入 RFBS 库存")
            self._ui_call(self._save_workspace_quietly)
            self.root.after(0, lambda: messagebox.showinfo(
                "自动流程完成",
                f"商品已完成 Ozon 导入并已请求写入 RFBS 库存。\n草稿：{draft_path}\n提交记录：{submitted_path}",
            ))
        except Exception:
            self._set_auto_progress(self.auto_progress_value, "自动流程已停止；请根据错误修正后重新运行")
            raise
        finally:
            self.auto_pipeline_running = False

    def _log(self, text):
        self.log_queue.put(str(text))

    def _poll_log(self):
        try:
            while True:
                text = self.log_queue.get_nowait()
                self.log_text.configure(state=tk.NORMAL)
                self.log_text.insert(tk.END, text + "\n")
                self.log_text.see(tk.END)
                self.log_text.configure(state=tk.DISABLED)
        except queue.Empty:
            pass
        self.root.after(100, self._poll_log)

    def _parse_url(self):
        reference = scrape_reference(self.vars["source_url"].get().strip(), log_func=self._log)
        if not reference.images:
            raise RuntimeError("Ozon 页面已读取到商品信息，但图库仍未加载完成；程序没有采用上一个商品的图片")
        paths, output_dir = self._download_reference_images(reference)
        self.reference = reference
        self._replace_images(paths, set_source=True)
        self.uploaded_urls = []
        self.root.after(0, lambda: (
            self.vars["source_url"].set(reference.source_url),
            self.vars["title"].set(reference.title),
            self.vars["tags"].set(format_ozon_hashtags(reference.tags)),
        ))
        self.root.after(0, lambda: (self.description_text.delete("1.0", tk.END), self.description_text.insert("1.0", reference.description)))
        article = reference.properties.get("Артикул", "未识别")
        self._log(f"已锁定单一变体 Артикул {article}；标题：{reference.title}；参考图 {len(reference.images)} 张；商品参数 {len(reference.properties)} 项；#标签 {len(reference.tags)} 个")
        self._log(f"高清原图下载并去重完成，共 {len(paths)} 张；目录：{output_dir}")

    def _prepare_local_job_images(self, job=None, *, analyze=False):
        if job is not None:
            paths = [
                str(path) for path in ((job.get("inputs") or {}).get("local_image_paths") or [])
                if str(path).strip()
            ]
        else:
            paths = list(self.source_images or self.local_images)
        paths = paths[:15]
        if not paths:
            raise ValueError("本地新品模式请至少选择 1 张成品图片")
        missing = [path for path in paths if not Path(path).is_file()]
        if missing:
            raise FileNotFoundError("本地新品图片不存在：" + "、".join(missing[:3]))
        self.reference = ReferenceProduct("", "")
        self._replace_images(paths, set_source=True)
        self.uploaded_urls = []
        self._log(f"本地新品已读取 {len(paths)} 张成品图；第 1 张作为 Ozon 主图")
        if analyze:
            return self._ensure_vision_analysis()
        return paths

    @staticmethod
    def _reference_from_local_vision(analysis: dict) -> ReferenceProduct:
        facts = [
            fact for fact in (analysis.get("visual_facts") or [])
            if isinstance(fact, dict)
            and str(fact.get("fact_name_ru") or "").strip()
            and str(fact.get("value_ru") or "").strip()
        ]
        title = str(analysis.get("product_name_ru") or "").strip()
        if not title:
            title = next((
                str(line).strip() for line in (analysis.get("poster_text_lines_ru") or [])
                if str(line).strip()
            ), "")
        if not title:
            raise RuntimeError("视觉分析未能可靠识别本地图片中的商品类型，请更换清晰主图后重试")
        properties = {}
        description_lines = []
        for fact in facts:
            name = str(fact["fact_name_ru"]).strip()
            value = str(fact["value_ru"]).strip()
            if name not in properties:
                properties[name] = value
            description_lines.append(f"{name}: {value}")
        return ReferenceProduct(
            "", title, "\n".join(description_lines), [], properties, [],
        )

    def _source_output_dir(self, reference=None) -> Path:
        selected = reference or self.reference
        match = re.search(r"(\d{6,})(?:/?(?:\?|#|$))", selected.source_url)
        key = selected.properties.get("Артикул") or (match.group(1) if match else uuid.uuid5(uuid.NAMESPACE_URL, selected.source_url).hex[:12])
        run_id = time.strftime("%Y%m%d-%H%M%S") + "-" + uuid.uuid4().hex[:6]
        return OUTPUT_DIR / "originals" / key / ("highres-" + run_id)

    def _download_reference_images(self, reference):
        if not reference.images:
            raise RuntimeError("Ozon 商品图库尚未加载完成，请等待浏览器解析后重试")
        output_dir = self._source_output_dir(reference)
        paths = ImageDownloadService().download(
            reference.images, str(output_dir), limit=15, log_func=self._log,
        )
        return paths, output_dir

    def _download_images(self):
        paths, output_dir = self._download_reference_images(self.reference)
        self._replace_images(paths, set_source=True)
        self.uploaded_urls = []
        self._log(f"高清原图下载并去重完成，共 {len(paths)} 张；目录：{output_dir}")

    def _select_images(self):
        paths = filedialog.askopenfilenames(title="选择商品图片", filetypes=[("图片", "*.jpg *.jpeg *.png *.webp")])
        for path in paths:
            if path not in self.local_images:
                self.local_images.append(path)
                self.source_images.append(path)
                self.image_list.insert(tk.END, path)
        if paths:
            self.vision_analysis = {}
            self.uploaded_urls = []
            self._save_workspace_quietly()

    def _remove_images(self):
        selected = tuple(self.image_list.curselection())
        for index in reversed(selected):
            removed = self.local_images.pop(index)
            if removed in self.source_images:
                self.source_images.remove(removed)
            self.image_list.delete(index)
        if selected:
            self.vision_analysis = {}
        self.uploaded_urls = []
        self.primary_reference_index = 0
        self._save_workspace_quietly()

    def _image_selection_changed(self, _event=None):
        selected = self.image_list.curselection()
        self.primary_reference_index = int(selected[0]) if len(selected) == 1 else 0
        self._save_workspace_quietly()

    def _select_watermark(self):
        path = filedialog.askopenfilename(title="选择水印", filetypes=[("图片", "*.png *.webp *.jpg")])
        if path:
            self.vars["watermark_path"].set(path)
            self._save_workspace_quietly()

    def _replace_images(self, paths, *, set_source=False):
        self.local_images = list(paths)
        self.primary_reference_index = 0
        if set_source:
            self.source_images = list(paths)
            self.vision_analysis = {}
        self.root.after(0, lambda: (self.image_list.delete(0, tk.END), [self.image_list.insert(tk.END, p) for p in paths]))

    def _ensure_vision_analysis(self):
        if self.vision_analysis:
            return self.vision_analysis
        references = self.source_images or self.local_images
        if not references:
            raise ValueError("产品视觉分析至少需要一张商品参考图")
        cfg = self._analysis_config()
        service = ProductVisionAnalysisService(
            cfg["api_url"], cfg["api_key"], cfg["model"],
        )
        analysis_cfg = self._config()["analysis"]
        if service.api_url != cfg["api_url"].rstrip("/"):
            target_key = "analysis_api_url" if analysis_cfg.get("api_url") else "text_api_url"
            self.root.after(0, lambda key=target_key: self.vars[key].set(service.api_url))
            self._log("视觉分析接口地址已自动补全为：" + service.api_url)
        self._log(f"视觉分析 AI 正在综合读取 {min(len(references), service.MAX_IMAGES)} 张商品图……")
        result = service.analyze(
            self.reference, references, listing_content=self._generated_content(),
        )
        self.vision_analysis = result
        self._ui_call(self._save_workspace_quietly)
        self._log(
            f"视觉分析完成：分析 {result.get('analyzed_image_count', 0)} 张图，"
            f"保留 {len(result.get('visual_facts', []))} 条带图片序号的视觉事实"
        )
        return result

    def _reanalyze_images(self):
        self.vision_analysis = {}
        result = self._ensure_vision_analysis()
        self._log(
            "视觉分析海报文字："
            + (" | ".join(result.get("poster_text_lines_ru", [])) or "未返回可用俄文文字，将使用页面事实兜底")
        )

    def _generate_images(self):
        references = self.source_images or self.local_images
        if not references:
            raise ValueError("请先解析链接下载原图，或手动选择参考图")
        watermark = self.vars["watermark_path"].get().strip()
        if not Path(watermark).is_file():
            raise FileNotFoundError("请先选择有效的水印文件")
        cfg = self._config()["image"]
        service = ImageGenerationService(cfg["api_url"], cfg["api_key"], cfg["model"])
        if service.api_url != cfg["api_url"].rstrip("/"):
            self.root.after(0, lambda: self.vars["image_api_url"].set(service.api_url))
            self._log("生图接口地址已自动补全为：" + service.api_url)
        primary_index = self.primary_reference_index if self.primary_reference_index < len(references) else 0
        primary_reference = references[primary_index]
        analysis = self._ensure_vision_analysis()
        poster_lines = [
            str(line).strip() for line in analysis.get("poster_text_lines_ru", [])
            if str(line).strip()
        ][:4]
        if not poster_lines:
            poster_lines = build_ozon_poster_text_lines(self.vars["title"].get(), self.reference)
        directions = [
            str(analysis.get("art_direction_en") or "").strip(),
            self.vars["image_prompt"].get().strip(),
        ]
        prompt = build_ozon_main_image_prompt(
            "\n".join(value for value in directions if value), poster_lines,
        )
        # As in the reference generator, the SKU cover receives exactly one
        # product-authority image.  Multiple uploads encourage variant mixing
        # and collage layouts; the remaining originals stay as attachments.
        generated = service.generate([primary_reference], prompt, 1, str(OUTPUT_DIR / "generated-main"))
        attachments = [
            path for index, path in enumerate(references)
            if index != primary_index
        ][:14]
        final_sources = [generated[0], *attachments]
        paths = WatermarkService.apply(final_sources, watermark, str(OUTPUT_DIR / "ready-to-upload"))
        self._replace_images(paths)
        self.uploaded_urls = []
        self._log(
            f"主图已锁定第 {primary_index + 1} 张原图为唯一 SKU 主体参考，按俄文图文海报规则生成并加水印；"
            f"海报文字：{' | '.join(poster_lines) or '无可用俄文事实'}；"
            f"视觉分析已参考 {analysis.get('analyzed_image_count', 0)} 张原图；"
            f"另保留并处理原附图 {len(paths) - 1} 张"
        )

    def _prepare_local_images_for_upload(self):
        references = list(self.source_images or self.local_images)[:15]
        if not references:
            raise ValueError("本地新品模式请至少选择 1 张成品图片")
        primary_index = self.primary_reference_index if self.primary_reference_index < len(references) else 0
        ordered = [references[primary_index], *[
            path for index, path in enumerate(references) if index != primary_index
        ]]
        watermark = self.vars["watermark_path"].get().strip()
        if watermark:
            if not Path(watermark).is_file():
                raise FileNotFoundError("已填写的水印文件不存在；请重新选择或留空")
            paths = WatermarkService.apply(
                ordered, watermark, str(OUTPUT_DIR / "local-ready-to-upload"),
            )
            message = f"本地新品 {len(paths)} 张成品图已添加水印，未调用图片生成服务"
        else:
            paths = ordered
            message = f"本地新品 {len(paths)} 张成品图直接用于上传，未添加水印且未调用图片生成服务"
        self._replace_images(paths)
        self.uploaded_urls = []
        self._log(message)

    def _generate_copywriting(self):
        if self._active_listing_is_local():
            self.reference = self._reference_from_local_vision(
                self._ensure_vision_analysis(),
            )
        elif not self.reference.title:
            raise ValueError("请先解析 Ozon 商品链接")
        cfg = self._config()["text"]
        service = CopywritingService(cfg["api_url"], cfg["api_key"], cfg["model"])
        if service.api_url != cfg["api_url"].rstrip("/"):
            self.root.after(0, lambda: self.vars["text_api_url"].set(service.api_url))
            self._log("文案接口地址已自动补全为：" + service.api_url)
        result = service.generate(self.reference)
        def update_fields():
            self.vars["title"].set(result["title_ru"])
            self.description_text.delete("1.0", tk.END)
            self.description_text.insert("1.0", result["description_ru"])
            self.vars["tags"].set(format_ozon_hashtags(result["tags_ru"]))
            self._prefill_content_attributes()
        self.root.after(0, update_fields)
        self._log(f"俄文标题、描述和 {len(result['tags_ru'])} 个标签已生成；请人工核对商品事实")

    def _watermark_images(self):
        paths = WatermarkService.apply(self.local_images, self.vars["watermark_path"].get(), str(OUTPUT_DIR / "watermarked"))
        self._replace_images(paths)
        self.uploaded_urls = []
        self._log(f"已添加水印 {len(paths)} 张")

    def _upload_images(self):
        offer_id = self.vars["offer_id"].get().strip()
        self.uploaded_urls = OssService(self._config()["oss"]).upload(self.local_images, offer_id)
        self._log(f"OSS 已按日期和货号归档：{time.strftime('%Y-%m-%d')}/{offer_id}")
        self._log("OSS 图片地址：\n" + "\n".join(self.uploaded_urls))

    @staticmethod
    def _merge_localized_records(primary: list[dict], russian: list[dict], *, category=False) -> list[dict]:
        if category:
            key = lambda item: (int(item["description_category_id"]), int(item["type_id"]))
        else:
            key = lambda item: int(item["id"])
        russian_by_id = {key(item): item for item in russian}
        merged = []
        for item in primary:
            localized = dict(item)
            ru = russian_by_id.get(key(item), {})
            localized["name_ru"] = str(ru.get("name") or "")
            localized["description_ru"] = str(ru.get("description") or "")
            localized["group_name_ru"] = str(ru.get("group_name") or "")
            if category:
                localized["display_ru"] = str(ru.get("display") or "")
                localized["path_ru"] = list(ru.get("path") or [])
            merged.append(localized)
        return merged

    def _localized_categories(self, api: OzonApi) -> list[dict]:
        chinese = flatten_categories(api.category_tree("ZH_HANS"))
        russian = flatten_categories(api.category_tree("RU"))
        return self._merge_localized_records(chinese, russian, category=True)

    @staticmethod
    def _valid_category_records(records) -> list[dict]:
        if not isinstance(records, list):
            return []
        valid_by_id: dict[tuple[int, int], dict] = {}
        for raw in records:
            if not isinstance(raw, dict):
                continue
            try:
                category_id = int(raw.get("description_category_id"))
                type_id = int(raw.get("type_id"))
            except (TypeError, ValueError):
                continue
            display = str(raw.get("display") or "").strip()
            if category_id <= 0 or type_id <= 0 or not display:
                continue
            item = dict(raw)
            item["description_category_id"] = category_id
            item["type_id"] = type_id
            item["display"] = display
            item["name"] = str(raw.get("name") or "").strip()
            item["name_ru"] = str(raw.get("name_ru") or "").strip()
            item["display_ru"] = str(raw.get("display_ru") or "").strip()
            item["path"] = [
                str(value or "").strip() for value in raw.get("path", [])
                if str(value or "").strip()
            ] if isinstance(raw.get("path"), list) else []
            item["path_ru"] = [
                str(value or "").strip() for value in raw.get("path_ru", [])
                if str(value or "").strip()
            ] if isinstance(raw.get("path_ru"), list) else []
            valid_by_id[(category_id, type_id)] = item
        return sorted(valid_by_id.values(), key=lambda item: item["display"].casefold())

    @staticmethod
    def _category_search_text(item: dict) -> str:
        values = [
            item.get("display"), item.get("display_ru"),
            item.get("name"), item.get("name_ru"),
            " ".join(item.get("path") or []),
            " ".join(item.get("path_ru") or []),
        ]
        return " ".join(str(value or "") for value in values).casefold().replace("ё", "е")

    def _apply_categories(self, records, *, source_label="") -> list[dict]:
        categories = self._valid_category_records(records)
        if not categories:
            return []
        category_by_display = {item["display"]: item for item in categories}
        displays = list(category_by_display)
        search_index = {
            display: self._category_search_text(item)
            for display, item in category_by_display.items()
        }
        self.categories = categories
        self.category_by_display = category_by_display
        self.category_displays = displays
        self.category_search_text_by_display = search_index

        def update():
            self.category_combo.configure(values=displays)
            self.work_category_combo.configure(values=displays)
            current_display = self.vars["category_display"].get().strip()
            if current_display in category_by_display:
                self._category_selected()
            job_display = self.job_form_vars["category_display"].get().strip()
            if job_display in category_by_display:
                self._job_category_selected()

        self.root.after(0, update)
        if source_label:
            self._log(f"已从{source_label}读取 {len(categories)} 个可用商品类型")
        return categories

    def _save_category_cache(self, records) -> Path:
        categories = self._valid_category_records(records)
        if not categories:
            raise ValueError("Ozon 类目为空，不会覆盖现有本地缓存")
        return atomic_write_json(CATEGORY_CACHE_PATH, {
            "version": 1,
            "saved_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            "languages": ["ZH_HANS", "RU"],
            "categories": categories,
        })

    def _load_category_cache(self) -> bool:
        payload = load_json(CATEGORY_CACHE_PATH, {})
        if not isinstance(payload, dict):
            return False
        categories = self._apply_categories(
            payload.get("categories"), source_label="本地类目缓存",
        )
        if not categories:
            return False
        saved_at = str(payload.get("saved_at") or "").strip()
        if saved_at:
            self._log("类目缓存更新时间：" + saved_at)
        return True

    def _localized_attributes(self, api: OzonApi, category_id: int, type_id: int) -> list[dict]:
        chinese = api.attributes(category_id, type_id, "ZH_HANS")
        russian = api.attributes(category_id, type_id, "RU")
        return self._merge_localized_records(chinese, russian)

    def _load_categories(self):
        categories = self._valid_category_records(self._localized_categories(self._api()))
        if not categories:
            raise RuntimeError("Ozon 没有返回可用类目，本地缓存未被覆盖")
        cache_path = self._save_category_cache(categories)
        self._apply_categories(categories, source_label="Ozon 实时类目")
        self._log(f"类目已缓存到本地：{cache_path}")
        self._log("请在自动工作台人工指定类目，AI 不会改类目")

    def _load_categories_with_retry(self):
        return self._retry_step("Ozon 类目读取", self._load_categories, attempts=3)

    @staticmethod
    def _matching_category_displays(
        query: str, displays: list[str], search_text_by_display: dict[str, str],
    ) -> list[str]:
        normalized = str(query or "").strip().casefold().replace("ё", "е")
        if not normalized:
            return list(displays)
        words = [word for word in re.split(r"\s+", normalized) if word]
        return [
            display for display in displays
            if all(
                word in search_text_by_display.get(
                    display, display.casefold().replace("ё", "е"),
                )
                for word in words
            )
        ]

    @staticmethod
    def _post_category_dropdown(widget):
        try:
            if widget.winfo_exists():
                widget.tk.call("ttk::combobox::Post", str(widget))
        except (AttributeError, tk.TclError):
            try:
                widget.event_generate("<Down>")
            except (AttributeError, tk.TclError):
                pass

    def _filter_category_choices(self, event=None):
        if event is not None and getattr(event, "keysym", "") in {
            "Up", "Down", "Left", "Right", "Return", "Escape", "Tab",
        }:
            return
        widget = getattr(event, "widget", None) if event is not None else None
        if widget is None or not hasattr(widget, "get") or not hasattr(widget, "configure"):
            widget = self.work_category_combo
        query = widget.get().strip()
        values = self._matching_category_displays(
            query, self.category_displays, self.category_search_text_by_display,
        )
        widget.configure(values=values)
        if query and values:
            try:
                widget.after_idle(lambda target=widget: self._post_category_dropdown(target))
            except (AttributeError, tk.TclError):
                pass

    def _category_selected(self):
        display = self.vars["category_display"].get().strip()
        category = self.category_by_display.get(display)
        if category:
            previous = (self.vars["category_id"].get(), self.vars["type_id"].get())
            current = (str(category["description_category_id"]), str(category["type_id"]))
            self.vars["category_id"].set(str(category["description_category_id"]))
            self.vars["type_id"].set(str(category["type_id"]))
            self.category_combo.set(display)
            if previous != current and self.attribute_definitions:
                self.attribute_definitions = []
                self.attribute_definition_by_id = {}
                self.attribute_fact_suggestions = {}
                self.attribute_text.delete("1.0", tk.END)
                self.attribute_text.insert("1.0", "[]")
                self.complex_attribute_text.delete("1.0", tk.END)
                self.complex_attribute_text.insert("1.0", "[]")
                self._refresh_attribute_tree()
                self.required_label.configure(text="类目已改变，请重新读取该类目的实时属性")

    @staticmethod
    def _is_model_name_label(name: str) -> bool:
        value = normalize_attribute_name(name)
        return (
            value in {"название модели", "объединить на одной карточке", "model name"}
            or value.startswith("型号名称")
        )

    @classmethod
    def _is_model_name_attribute(cls, definition: dict) -> bool:
        return cls._is_model_name_label(definition.get("name")) or cls._is_model_name_label(definition.get("name_ru"))

    @staticmethod
    def _is_country_of_origin_attribute(definition: dict) -> bool:
        names = {
            normalize_attribute_name(definition.get("name")),
            normalize_attribute_name(definition.get("name_ru")),
        }
        names.discard("")
        aliases = {
            "原产国", "生产国", "制造国",
            "страна изготовитель", "страна производства",
            "страна происхождения", "страна происхождения товара",
            "country of origin", "country of manufacture",
        }
        return bool(names & aliases)

    @staticmethod
    def _system_default_attribute_kind(definition: dict) -> str:
        names = {
            normalize_attribute_name(definition.get("name")),
            normalize_attribute_name(definition.get("name_ru")),
        }
        names.discard("")
        if names & {
            "一个商品中的件数", "единиц в одном товаре",
            "количество единиц в одном товаре",
        }:
            return "units_per_product"
        if names & {"原厂包装数量", "количество заводских упаковок"}:
            return "factory_package_count"
        if names & {"保证", "保修期", "гарантия", "гарантийный срок"}:
            return "warranty"
        if RfbsListingApp._is_country_of_origin_attribute(definition):
            return "country_of_origin"
        return ""

    @staticmethod
    def _one_year_warranty_option(options: list[dict]) -> dict | None:
        aliases = {"1年", "1год", "12месяцев", "oneyear"}
        for option in options:
            value = re.sub(r"[\s._-]+", "", str(option.get("value") or "").casefold().replace("ё", "е"))
            if value in aliases and int(option.get("id") or 0):
                return option
        return None

    @staticmethod
    def _china_country_option(options: list[dict]) -> dict | None:
        aliases = {
            "中国", "中国大陆", "中华人民共和国",
            "китай", "кнр", "китайская народная республика",
            "china", "people s republic of china", "people's republic of china",
        }
        for option in options:
            value = normalize_attribute_name(option.get("value"))
            if value in aliases and int(option.get("id") or 0):
                return option
        return None

    def _apply_system_attribute_default(
        self,
        attributes: list[dict],
        complex_attributes: list[dict],
        definition: dict,
        *,
        dictionary_options: list[dict] | None = None,
    ) -> bool:
        """Fill a system default only when this live attribute has no value."""
        kind = self._system_default_attribute_kind(definition)
        if not kind:
            return False
        existing = attribute_values_by_id(attributes, complex_attributes).get(int(definition["id"]))
        if existing and any(
            value.get("value") or value.get("dictionary_value_id")
            for value in existing.get("values", [])
        ):
            return False
        if kind in {"warranty", "country_of_origin"}:
            if not int(definition.get("dictionary_id") or 0):
                label = "保证" if kind == "warranty" else "原产国"
                raise ValueError(f"Ozon 实时“{label}”属性没有字典 ID，无法安全填写规定值")
            option = (
                self._one_year_warranty_option(dictionary_options or [])
                if kind == "warranty"
                else self._china_country_option(dictionary_options or [])
            )
            if not option:
                expected = "1 年" if kind == "warranty" else "中国"
                label = "保证" if kind == "warranty" else "原产国"
                raise ValueError(
                    f"Ozon“{label}”实时字典中没有找到“{expected}”，"
                    "请刷新该属性字典后重试"
                )
            expected = "1 年" if kind == "warranty" else "中国"
            source = (
                "系统默认：保证 1 年（Ozon 实时字典）"
                if kind == "warranty"
                else "系统默认：原产国 中国（Ozon 实时字典）"
            )
            set_attribute_value(
                attributes, complex_attributes, definition,
                value=str(option.get("value") or expected),
                dictionary_value_id=int(option["id"]),
                source=source,
            )
            return True
        if int(definition.get("dictionary_id") or 0):
            raise ValueError(f"Ozon 将属性“{definition.get('name')}”返回成了字典，无法直接填写默认值 1")
        source = (
            "系统默认：一个商品中的件数 1"
            if kind == "units_per_product"
            else "系统默认：原厂包装数量 1"
        )
        set_attribute_value(
            attributes, complex_attributes, definition,
            value="1", source=source,
        )
        return True

    def _enforce_system_defaults_before_submission(
        self, attributes: list[dict], complex_attributes: list[dict],
    ) -> int:
        """Keep explicit repairs, but require every applicable default before submission."""
        changed = 0
        for definition in self.attribute_definitions:
            kind = self._system_default_attribute_kind(definition)
            if not kind:
                continue
            existing = attribute_values_by_id(attributes, complex_attributes).get(int(definition["id"]))
            has_value = bool(existing and any(
                value.get("value") or value.get("dictionary_value_id")
                for value in existing.get("values", [])
            ))
            if has_value:
                continue
            if kind in {"warranty", "country_of_origin"}:
                label = "保证 1 年" if kind == "warranty" else "原产国 中国"
                raise ValueError(
                    f"“{label}”缺少 Ozon 实时字典值；请重新运行类目属性填写，"
                    "程序会从 Ozon 实时字典选择有效 dictionary_value_id"
                )
            if self._apply_system_attribute_default(attributes, complex_attributes, definition):
                changed += 1
        return changed

    @staticmethod
    def _content_attribute_kind(name: str) -> str:
        value = re.sub(r"\s+", " ", str(name).casefold().replace("ё", "е")).strip()
        if RfbsListingApp._is_model_name_label(name):
            return "model_name"
        if value in {"название", "名称"}:
            return "title"
        if value in {"аннотация", "описание", "описание товара", "描述", "商品描述", "产品描述"} or "описание товара" in value:
            return "description"
        if any(phrase in value for phrase in (
            "ключевые слова", "поисковые теги", "поисковые запросы", "关键词", "搜索标签", "搜索词", "主题标签",
        )) or value in {"теги", "#хештеги"}:
            return "tags"
        return ""

    def _content_value(self, kind: str) -> str:
        if kind == "title":
            return self.vars["title"].get().strip()
        if kind == "description":
            return self.description_text.get("1.0", tk.END).strip()
        if kind == "tags":
            return format_ozon_hashtags(self.vars["tags"].get())
        if kind == "model_name":
            variable = self.vars.get("model_name")
            return variable.get().strip() if variable else ""
        return ""

    def _prefill_content_attributes(self):
        """Fill only empty live category text fields, preserving manual edits."""
        try:
            attributes = parse_attributes(self.attribute_text.get("1.0", tk.END))
            complex_attributes = parse_complex_attributes(self.complex_attribute_text.get("1.0", tk.END))
        except (ValueError, json.JSONDecodeError):
            return
        changed = False
        values_by_id = attribute_values_by_id(attributes, complex_attributes)
        for definition in self.attribute_definitions:
            kind = self._content_attribute_kind(definition.get("name", "")) or self._content_attribute_kind(definition.get("name_ru", ""))
            generated = self._content_value(kind)
            if not kind or not generated:
                continue
            item = values_by_id.get(int(definition["id"]))
            current = ""
            if item:
                current = str(next((value.get("value") or value.get("dictionary_value_id") for value in item.get("values", []) if value.get("value") or value.get("dictionary_value_id")), "")).strip()
            if not current:
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=generated,
                    source="人工填写：型号名称" if kind == "model_name" else "生成文案（待核对）",
                )
                changed = True
        if changed:
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()

    def _read_attribute_documents(self):
        return (
            parse_attributes(self.attribute_text.get("1.0", tk.END)),
            parse_complex_attributes(self.complex_attribute_text.get("1.0", tk.END)),
        )

    def _write_attribute_documents(self, attributes, complex_attributes):
        self.attribute_text.delete("1.0", tk.END)
        self.attribute_text.insert("1.0", json.dumps(attributes, ensure_ascii=False, indent=2))
        self.complex_attribute_text.delete("1.0", tk.END)
        self.complex_attribute_text.insert("1.0", json.dumps(complex_attributes, ensure_ascii=False, indent=2))

    @staticmethod
    def _display_attribute_value(item: dict | None) -> str:
        if not item:
            return ""
        rendered = []
        for value in item.get("values", []):
            text = str(value.get("value") or "").strip()
            dictionary_id = int(value.get("dictionary_value_id") or 0)
            if dictionary_id:
                rendered.append(f"{text} [ID={dictionary_id}]" if text else f"ID={dictionary_id}")
            elif text:
                rendered.append(text)
        result = "; ".join(rendered)
        return result if len(result) <= 260 else result[:257] + "…"

    def _refresh_attribute_tree(self):
        selected_before = self.attribute_tree.selection()
        for item_id in self.attribute_tree.get_children():
            self.attribute_tree.delete(item_id)
        if not self.attribute_definitions:
            return
        try:
            attributes, complex_attributes = self._read_attribute_documents()
        except (ValueError, json.JSONDecodeError):
            self.required_label.configure(text="高级 JSON 格式错误，属性表暂时无法刷新")
            return
        current = attribute_values_by_id(attributes, complex_attributes)
        required = [item for item in self.attribute_definitions if item.get("is_required")]
        missing = validate_required(attributes, self.attribute_definitions, complex_attributes)
        definitions = sorted(
            self.attribute_definitions,
            key=lambda item: (not bool(item.get("is_required")), str(item.get("group_name") or ""), str(item.get("name") or "").casefold()),
        )
        for definition in definitions:
            attribute_id = int(definition["id"])
            item = current.get(attribute_id)
            value = self._display_attribute_value(item)
            suggestion = self.attribute_fact_suggestions.get(attribute_id)
            source = str((item or {}).get("_source") or "")
            if not value and suggestion:
                if int(definition.get("dictionary_id") or 0):
                    value = "未选择"
                    source = suggestion[0]
                    if suggestion[1]:
                        source += "；内部检索词：" + suggestion[1]
                else:
                    value = "候选：" + suggestion[1]
                    source = suggestion[0]
            if not value and definition.get("is_required"):
                source = "待填写"
            kind = "字典" if int(definition.get("dictionary_id") or 0) else "文本"
            if definition.get("is_collection"):
                kind += "（可多值）"
            if int(definition.get("attribute_complex_id") or 0):
                kind += " / 组合"
            self.attribute_tree.insert(
                "", "end", iid=str(attribute_id), text=str(definition.get("name") or attribute_id),
                values=("是" if definition.get("is_required") else "", kind, value, source),
            )
        if selected_before and self.attribute_tree.exists(selected_before[0]):
            self.attribute_tree.selection_set(selected_before[0])
        filled_required = len(required) - len(missing)
        self.required_label.configure(
            text=f"实时属性 {len(self.attribute_definitions)} 项；必填 {len(required)} 项，已填 {filled_required} 项，仍缺 {len(missing)} 项"
            + (("：" + "、".join(missing[:8]) + ("……" if len(missing) > 8 else "")) if missing else "；可生成草稿并继续复核")
        )

    def _selected_attribute_definition(self):
        selected = self.attribute_tree.selection()
        if len(selected) != 1:
            return None
        try:
            return self.attribute_definition_by_id.get(int(selected[0]))
        except (TypeError, ValueError):
            return None

    def _attribute_tree_selected(self):
        definition = self._selected_attribute_definition()
        if not definition:
            return
        try:
            attributes, complex_attributes = self._read_attribute_documents()
            item = attribute_values_by_id(attributes, complex_attributes).get(int(definition["id"]))
        except (ValueError, json.JSONDecodeError):
            item = None
        raw_value = ""
        if item and item.get("values"):
            raw_value = str(item["values"][0].get("value") or "")
        suggestion = self.attribute_fact_suggestions.get(int(definition["id"]))
        self.attribute_value_var.set(raw_value)
        self.dictionary_query_var.set(raw_value or (suggestion[1] if suggestion else ""))
        self.attribute_dictionary_results = {}
        self.attribute_dictionary_all_results = []
        self.attribute_dictionary_attribute_id = 0
        self.dictionary_combo.set("")
        self.dictionary_combo.configure(values=[])
        required = "必填" if definition.get("is_required") else "选填"
        description = re.sub(r"\s+", " ", str(definition.get("description") or "")).strip()
        suffix = f"；{description}" if description else ""
        self.selected_attribute_label.configure(
            text=f"{definition.get('name')}（ID {definition.get('id')}，{required}）{suffix}"
        )
        if int(definition.get("dictionary_id") or 0):
            if self._is_brand_attribute(definition):
                locked = self._locked_no_brand_dictionary_value()
                display = self._dictionary_option_display(locked)
                self.attribute_dictionary_all_results = [locked]
                self.attribute_dictionary_results = {display: locked}
                self.attribute_dictionary_attribute_id = int(definition["id"])
                self.dictionary_combo.configure(values=[display])
                self.dictionary_combo.set(display)
                self.dictionary_status_label.configure(text="品牌已锁死为无品牌，不加载品牌库，也不允许改成其他品牌")
                return
            cache_path = self.dictionary_cache.path_for(
                int(self.vars["category_id"].get()), int(self.vars["type_id"].get()),
                int(definition["id"]), "ZH_HANS",
            )
            self.dictionary_status_label.configure(
                text="已有本地缓存，点击“加载全部选项”可立即读取"
                if cache_path.is_file() else
                "尚无缓存，点击“加载全部选项”后会读取 Ozon 全部分页值"
            )
        else:
            self.dictionary_status_label.configure(text="该属性是自由文本，必须填写俄文")

    def _save_selected_attribute_text(self):
        definition = self._selected_attribute_definition()
        if not definition:
            messagebox.showwarning("未选择属性", "请先在属性表中选择一项")
            return
        if int(definition.get("dictionary_id") or 0):
            messagebox.showwarning("必须选择字典值", "该属性不能直接输入文本，请加载全部 Ozon 选项并采用返回值")
            return
        value = self.attribute_value_var.get().strip()
        if not value:
            messagebox.showwarning("属性值为空", "请输入真实属性值")
            return
        try:
            attributes, complex_attributes = self._read_attribute_documents()
            set_attribute_value(attributes, complex_attributes, definition, value=value, source="人工填写")
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()
            self._save_workspace_quietly()
        except (ValueError, json.JSONDecodeError) as error:
            messagebox.showerror("属性 JSON 错误", str(error))

    def _begin_dictionary_load(self, force_refresh=False):
        definition = self._selected_attribute_definition()
        if not definition:
            messagebox.showwarning("未选择属性", "请先在属性表中选择一项")
            return
        if not int(definition.get("dictionary_id") or 0):
            messagebox.showwarning("不是字典属性", "该属性可直接填写文本值")
            return
        attribute_id = int(definition["id"])
        if self._is_brand_attribute(definition):
            locked = self._locked_no_brand_dictionary_value()
            display = self._dictionary_option_display(locked)
            self.attribute_dictionary_all_results = [locked]
            self.attribute_dictionary_results = {display: locked}
            self.attribute_dictionary_attribute_id = attribute_id
            self.dictionary_combo.configure(values=[display])
            self.dictionary_combo.set(display)
            self.dictionary_status_label.configure(text="品牌已锁死为无品牌，不会加载品牌库")
            return
        self.dictionary_status_label.configure(text="正在读取全部字典选项……")
        self._run(lambda: self._load_dictionary_values(attribute_id, force_refresh=force_refresh))

    def _clear_selected_attribute(self):
        definition = self._selected_attribute_definition()
        if not definition:
            messagebox.showwarning("未选择属性", "请先在属性表中选择一项")
            return
        if self._is_brand_attribute(definition):
            messagebox.showwarning("品牌已锁定", "品牌固定为“无品牌 / Нет бренда”，不能清除或修改")
            return
        try:
            attributes, complex_attributes = self._read_attribute_documents()
            attribute_id = int(definition["id"])
            attributes[:] = [item for item in attributes if int(item.get("id") or 0) != attribute_id]
            for group in complex_attributes:
                group["attributes"] = [item for item in group.get("attributes", []) if int(item.get("id") or 0) != attribute_id]
            complex_attributes[:] = [group for group in complex_attributes if group.get("attributes")]
            self.attribute_fact_suggestions.pop(attribute_id, None)
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()
            self._save_workspace_quietly()
        except (ValueError, json.JSONDecodeError) as error:
            messagebox.showerror("属性 JSON 错误", str(error))

    @staticmethod
    def _dictionary_option_display(item: dict) -> str:
        value = str(item.get("value") or "").strip()
        info = str(item.get("info") or "").strip()
        if int(item.get("id") or 0) == NO_BRAND_DICTIONARY_ID or value.casefold() == NO_BRAND_DICTIONARY_VALUE.casefold():
            value = "无品牌 / Нет бренда"
        suffix = f" | {info}" if info and info.casefold() not in value.casefold() else ""
        return f"{value}{suffix} | ID={item.get('id')}"

    @staticmethod
    def _locked_no_brand_dictionary_value() -> dict:
        return {
            "id": NO_BRAND_DICTIONARY_ID,
            "value": NO_BRAND_DICTIONARY_VALUE,
            "info": "Товар не имеет бренда",
        }

    def _filter_dictionary_choices(self, _event=None):
        definition = self._selected_attribute_definition()
        if definition and self._is_brand_attribute(definition):
            locked = self._locked_no_brand_dictionary_value()
            display = self._dictionary_option_display(locked)
            self.attribute_dictionary_all_results = [locked]
            self.attribute_dictionary_results = {display: locked}
            self.attribute_dictionary_attribute_id = int(definition["id"])
            self.dictionary_combo.configure(values=[display])
            self.dictionary_combo.set(display)
            self.dictionary_status_label.configure(text="品牌已锁死为无品牌，不加载品牌库，也不允许改成其他品牌")
            return
        query = self.dictionary_query_var.get().strip().casefold()
        matches = []
        for item in self.attribute_dictionary_all_results:
            haystack = " ".join((
                self._dictionary_option_display(item),
                str(item.get("value") or ""), str(item.get("info") or ""), str(item.get("id") or ""),
            )).casefold()
            if not query or query in haystack:
                matches.append(item)
        visible = matches[:2000]
        mapping = {self._dictionary_option_display(item): item for item in visible}
        self.attribute_dictionary_results = mapping
        self.dictionary_combo.configure(values=list(mapping))
        self.dictionary_combo.set(next(iter(mapping), ""))
        truncated = "，下拉框先显示前 2000 项，请输入关键词本地筛选" if len(matches) > len(visible) else ""
        self.dictionary_status_label.configure(
            text=f"缓存共 {len(self.attribute_dictionary_all_results)} 项；当前匹配 {len(matches)} 项{truncated}"
        )

    def _cached_dictionary_values(self, api: OzonApi, category_id: int, type_id: int, attribute_id: int, *, force_refresh=False):
        values, from_cache, path = self.dictionary_cache.load(
            api, category_id, type_id, attribute_id, "ZH_HANS",
            force_refresh=force_refresh, log_func=self._log,
        )
        return values, from_cache, path

    def _load_dictionary_values(self, attribute_id: int, *, force_refresh=False):
        definition = self.attribute_definition_by_id.get(int(attribute_id))
        if definition and self._is_brand_attribute(definition):
            self._log("品牌已锁死为无品牌，已跳过 Ozon 品牌字典请求")
            return
        category_id = int(self.vars["category_id"].get())
        type_id = int(self.vars["type_id"].get())
        results, from_cache, path = self._cached_dictionary_values(
            self._api(), category_id, type_id, attribute_id, force_refresh=force_refresh,
        )
        def update():
            selected = self._selected_attribute_definition()
            if not selected or int(selected["id"]) != attribute_id:
                return
            self.attribute_dictionary_all_results = results
            self.attribute_dictionary_attribute_id = attribute_id
            self._filter_dictionary_choices()
        self.root.after(0, update)
        source = "本地缓存" if from_cache else "Ozon 全部分页"
        self._log(f"属性 {attribute_id} 已从{source}加载 {len(results)} 个字典选项；缓存：{path}")

    def _apply_dictionary_choice(self):
        definition = self._selected_attribute_definition()
        choice = self.attribute_dictionary_results.get(self.dictionary_combo.get())
        if not definition or int(definition["id"]) != self.attribute_dictionary_attribute_id or not choice:
            messagebox.showwarning("未选择字典值", "请先加载全部选项，再从本地筛选结果中选择")
            return
        if self._is_brand_attribute(definition):
            choice = self._locked_no_brand_dictionary_value()
        try:
            attributes, complex_attributes = self._read_attribute_documents()
            set_attribute_value(
                attributes, complex_attributes, definition,
                value=str(choice.get("value") or ""), dictionary_value_id=int(choice["id"]),
                source="Ozon 字典（人工确认）", append=bool(definition.get("is_collection")),
            )
            self.attribute_fact_suggestions.pop(int(definition["id"]), None)
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()
            self._save_workspace_quietly()
        except (ValueError, json.JSONDecodeError, KeyError) as error:
            messagebox.showerror("属性保存失败", str(error))

    @staticmethod
    def _exact_dictionary_value(results: list[dict], candidate: str):
        wanted = normalize_attribute_name(candidate)
        exact = [item for item in results if normalize_attribute_name(item.get("value")) == wanted]
        return exact[0] if len(exact) == 1 else None

    @staticmethod
    def _is_brand_attribute(definition: dict) -> bool:
        names = {
            normalize_attribute_name(definition.get("name")),
            normalize_attribute_name(definition.get("name_ru")),
        }
        return int(definition.get("id") or 0) == 85 or bool(names & {"品牌", "бренд", "бренд товара"})

    @staticmethod
    def _is_product_weight_attribute(definition: dict) -> bool:
        names = {
            normalize_attribute_name(definition.get("name")),
            normalize_attribute_name(definition.get("name_ru")),
        }
        return int(definition.get("id") or 0) == 4383 or bool(names & {"商品重量 克", "вес г"})

    @staticmethod
    def _is_product_type_attribute(definition: dict) -> bool:
        names = {
            normalize_attribute_name(definition.get("name")),
            normalize_attribute_name(definition.get("name_ru")),
        }
        return int(definition.get("id") or 0) == 8229 or bool(names & {"类型", "тип"})

    def _enforce_unique_required_product_type(
        self,
        attributes: list[dict],
        complex_attributes: list[dict],
        *,
        api=None,
        category_id: int | None = None,
        type_id: int | None = None,
        definitions: list[dict] | None = None,
    ) -> int:
        """Fill Ozon's required product type when the live dictionary has one choice."""
        current = attribute_values_by_id(attributes, complex_attributes)
        required_type_definitions = [
            definition for definition in (
                definitions if definitions is not None else self.attribute_definitions
            )
            if definition.get("is_required")
            and int(definition.get("dictionary_id") or 0)
            and self._is_product_type_attribute(definition)
            and int(definition.get("id") or 0) not in current
        ]
        if not required_type_definitions:
            return 0
        resolved_category_id = int(
            category_id if category_id is not None else self.vars["category_id"].get()
        )
        resolved_type_id = int(
            type_id if type_id is not None else self.vars["type_id"].get()
        )
        selected_api = api or self._api()
        filled = 0
        for definition in required_type_definitions:
            options, _, _ = self._cached_dictionary_values(
                selected_api, resolved_category_id, resolved_type_id,
                int(definition["id"]),
            )
            unique_options = {
                int(option["id"]): option
                for option in options
                if isinstance(option, dict)
                and option.get("id")
                and str(option.get("value") or "").strip()
            }
            if len(unique_options) != 1:
                continue
            option = next(iter(unique_options.values()))
            set_attribute_value(
                attributes, complex_attributes, definition,
                value=str(option["value"]), dictionary_value_id=int(option["id"]),
                source="系统自动：当前类目唯一可选类型",
            )
            filled += 1
        return filled

    def _enforce_locked_brand(self, attributes: list[dict], complex_attributes: list[dict]) -> bool:
        definition = next((item for item in self.attribute_definitions if self._is_brand_attribute(item)), None)
        if not definition:
            return False
        attribute_id = int(definition["id"])
        attributes[:] = [item for item in attributes if int(item.get("id") or 0) != attribute_id]
        for group in complex_attributes:
            group["attributes"] = [
                item for item in group.get("attributes", [])
                if int(item.get("id") or 0) != attribute_id
            ]
        complex_attributes[:] = [group for group in complex_attributes if group.get("attributes")]
        locked = self._locked_no_brand_dictionary_value()
        set_attribute_value(
            attributes, complex_attributes, definition,
            value=str(locked["value"]), dictionary_value_id=int(locked["id"]),
            source="系统锁定：无品牌",
        )
        return True

    def _validate_dictionary_values_before_submission(
        self,
        attributes: list[dict],
        complex_attributes: list[dict],
    ) -> int:
        """Require every dictionary value to exist in the current Ozon option list."""
        category_id = int(self.vars["category_id"].get())
        type_id = int(self.vars["type_id"].get())
        api = self._api()
        current = attribute_values_by_id(attributes, complex_attributes)
        repaired = 0

        for definition in self.attribute_definitions:
            if not int(definition.get("dictionary_id") or 0):
                continue
            attribute_id = int(definition["id"])
            item = current.get(attribute_id)
            if not item or not item.get("values"):
                continue
            label = str(definition.get("name") or definition.get("name_ru") or attribute_id)

            if self._is_brand_attribute(definition):
                locked = self._locked_no_brand_dictionary_value()
                expected_id = int(locked["id"])
                values = item.get("values") or []
                if (
                    len(values) != 1
                    or int(values[0].get("dictionary_value_id") or 0) != expected_id
                ):
                    raise ValueError("品牌只能使用系统锁定的 Ozon 字典值“无品牌”")
                values[0]["value"] = str(locked["value"])
                values[0]["dictionary_value_id"] = expected_id
                continue

            options, _, _ = self._cached_dictionary_values(
                api, category_id, type_id, attribute_id,
            )
            allowed: dict[int, dict] = {}
            for option in options:
                try:
                    option_id = int(option.get("id") or 0)
                except (TypeError, ValueError):
                    continue
                if option_id:
                    allowed[option_id] = option
            if not allowed:
                raise ValueError(f"属性“{label}”没有读取到任何 Ozon 实时可选值，不能安全提交")

            raw_values = item.get("values") or []
            maximum = int(definition.get("max_value_count") or 0)
            if not definition.get("is_collection") and len(raw_values) > 1:
                raise ValueError(f"字典属性“{label}”只能选择一个 Ozon 规定值")
            if maximum and len(raw_values) > maximum:
                raise ValueError(f"字典属性“{label}”最多允许选择 {maximum} 个值")

            validated = []
            seen_ids = set()
            repaired_before_attribute = repaired
            for raw_value in raw_values:
                if not isinstance(raw_value, dict):
                    raise ValueError(f"字典属性“{label}”的值格式错误")
                text_value = str(raw_value.get("value") or "").strip()
                try:
                    selected_id = int(raw_value.get("dictionary_value_id") or 0)
                except (TypeError, ValueError):
                    selected_id = 0
                option = allowed.get(selected_id)
                if option is None and text_value:
                    option = self._exact_dictionary_value(options, text_value)
                if option is None and self._is_country_of_origin_attribute(definition):
                    option = self._china_country_option(options)
                if option is None:
                    shown = text_value or (f"ID={selected_id}" if selected_id else "空值")
                    raise ValueError(
                        f"字典属性“{label}”中的“{shown}”不是当前 Ozon 可供选择的内容；"
                        "请在属性页加载全部选项后重新选择"
                    )
                option_id = int(option["id"])
                if option_id in seen_ids:
                    repaired += 1
                    continue
                canonical_value = str(option.get("value") or "").strip()
                if (
                    selected_id != option_id
                    or text_value != canonical_value
                    or set(raw_value) != {"value", "dictionary_value_id"}
                ):
                    repaired += 1
                validated.append({
                    "value": canonical_value,
                    "dictionary_value_id": option_id,
                })
                seen_ids.add(option_id)
            item["values"] = validated
            if repaired > repaired_before_attribute and item.get("_source"):
                item["_source"] = str(item["_source"]) + " + Ozon字典提交前校验"
        return repaired

    def _enforce_generated_content_attributes(
        self, attributes: list[dict], complex_attributes: list[dict], content: dict,
    ) -> int:
        """Keep Ozon title/description/hashtag attributes synchronized at submission time."""
        values = {
            "title": str(content.get("title_ru") or "").strip(),
            "description": str(content.get("description_ru") or "").strip(),
            "tags": format_ozon_hashtags(content.get("tags_ru") or []),
        }
        changed = 0
        for definition in self.attribute_definitions:
            kind = self._content_attribute_kind(definition.get("name", "")) or self._content_attribute_kind(definition.get("name_ru", ""))
            value = values.get(kind, "")
            if not kind or not value or int(definition.get("dictionary_id") or 0):
                continue
            set_attribute_value(
                attributes, complex_attributes, definition,
                value=value, source="提交前同步生成文案",
            )
            changed += 1
        return changed

    def _enforce_manual_net_weight(self, attributes: list[dict], complex_attributes: list[dict]) -> bool:
        definition = next((item for item in self.attribute_definitions if self._is_product_weight_attribute(item)), None)
        value = self.vars.get("net_weight").get().strip() if self.vars.get("net_weight") else ""
        if not definition or not value or int(definition.get("dictionary_id") or 0):
            return False
        set_attribute_value(
            attributes, complex_attributes, definition,
            value=value, source="自动工作台：商品净重",
        )
        return True

    def _enforce_manual_model_name(self, attributes: list[dict], complex_attributes: list[dict]) -> bool:
        value = self.vars.get("model_name").get().strip() if self.vars.get("model_name") else ""
        if not value:
            raise ValueError("型号名称不能为空；请在自动工作台人工填写")
        definition = next((item for item in self.attribute_definitions if self._is_model_name_attribute(item)), None)
        if not definition:
            raise ValueError(
                "当前 Ozon 实时类目没有返回“型号名称（针对合并为一张商品卡片）”属性，"
                "无法按型号合并变体；请检查所选类目"
            )
        if int(definition.get("dictionary_id") or 0):
            raise ValueError("Ozon 将型号名称返回成了字典属性，程序不能提交任意文本；请刷新类目属性后重试")
        set_attribute_value(
            attributes, complex_attributes, definition,
            value=value, source="人工填写：型号名称（合并多变体）",
        )
        return True

    @staticmethod
    def _rank_dictionary_options(results: list[dict], candidates: list[str], limit=40) -> list[dict]:
        normalized_candidates = [normalize_attribute_name(value) for value in candidates]
        normalized_candidates = [value for value in normalized_candidates if value]
        scored = []
        for item in results:
            normalized_value = normalize_attribute_name(item.get("value"))
            if not normalized_value:
                continue
            value_compact = normalized_value.replace(" ", "")
            best = 0.0
            for candidate in normalized_candidates:
                candidate_compact = candidate.replace(" ", "")
                if normalized_value == candidate:
                    best = max(best, 1000.0)
                    continue
                if candidate in normalized_value or normalized_value in candidate:
                    best = max(best, 500.0 + min(len(candidate), len(normalized_value)))
                    continue
                candidate_chars = set(candidate_compact)
                value_chars = set(value_compact)
                if not candidate_chars or not value_chars:
                    continue
                overlap = len(candidate_chars & value_chars) / max(len(candidate_chars), len(value_chars))
                if overlap >= 0.35:
                    best = max(best, overlap * 100.0)
            if best:
                scored.append((best, str(item.get("value") or "").casefold(), item))
        scored.sort(key=lambda row: (-row[0], row[1]))
        return [item for _, _, item in scored[:max(1, int(limit))]]

    @classmethod
    def _dictionary_options_for_ai(
        cls,
        results: list[dict],
        candidates: list[str],
        *,
        full_dictionary_limit=500,
        full_dictionary_char_limit=18000,
        ranked_limit=80,
    ) -> list[dict]:
        """Give AI only real Ozon options, using the complete list when it is bounded."""
        valid = [
            item for item in results
            if isinstance(item, dict)
            and item.get("id")
            and str(item.get("value") or "").strip()
        ]
        dictionary_chars = sum(
            len(str(item.get("value") or "")) + 20 for item in valid
        )
        if (
            len(valid) <= max(1, int(full_dictionary_limit))
            and dictionary_chars <= max(1000, int(full_dictionary_char_limit))
        ):
            return valid
        return cls._rank_dictionary_options(
            valid, candidates, limit=max(1, int(ranked_limit)),
        )

    def _ai_fill_selected_category_attributes(self):
        return self._ai_fill_category_attributes(use_selected_category=True)

    def _ai_fill_category_attributes(self, use_selected_category=False):
        if not self.reference.title:
            raise ValueError("请先读取指定 Ozon 商品")
        text_cfg = self._config()["text"]
        ai = CopywritingService(text_cfg["api_url"], text_cfg["api_key"], text_cfg["model"])
        if ai.api_url != text_cfg["api_url"].rstrip("/"):
            self.root.after(0, lambda: self.vars["text_api_url"].set(ai.api_url))
            self._log("AI 接口地址已自动补全为：" + ai.api_url)
        api = self._api()
        categories = self.categories or self._localized_categories(api)
        if not categories:
            raise RuntimeError("Ozon 没有返回可用类目")

        if use_selected_category:
            category_id = int(self.vars["category_id"].get())
            type_id = int(self.vars["type_id"].get())
            selected_category = next((
                item for item in categories
                if int(item["description_category_id"]) == category_id and int(item["type_id"]) == type_id
            ), None)
            if not selected_category:
                raise ValueError("手工选择的类目已不在 Ozon 实时类目树中，请刷新类目后重新选择")
            keywords = []
            self._log("采用人工指定类目，AI 不参与类目选择：" + selected_category["display"])
        else:
            self._log("AI 正在识别商品类型并从 Ozon 实时叶子类目中选择……")
            selected_category, keywords = ai.choose_category(self.reference, categories)
            category_id = int(selected_category["description_category_id"])
            type_id = int(selected_category["type_id"])
        definitions = self._localized_attributes(api, category_id, type_id)
        if not definitions:
            raise RuntimeError("指定类目没有返回实时属性，程序已停止")
        if not any(self._is_model_name_attribute(item) for item in definitions):
            raise ValueError(
                "当前 Ozon 实时类目没有“型号名称（针对合并为一张商品卡片）”属性，"
                "请检查类目是否支持合并多变体"
            )

        content = self._generated_content()
        visual_analysis = self._ensure_vision_analysis() if (self.source_images or self.local_images) else {}
        self._log(
            f"正在为类目 {selected_category['display']} 映射 {len(definitions)} 个实时属性；"
            f"可用视觉事实 {len(visual_analysis.get('visual_facts', []))} 条……"
        )
        ai_mappings = ai.map_attributes(
            self.reference, content, definitions, visual_analysis=visual_analysis,
        )
        image_mapping_count = sum(
            1 for item in ai_mappings if item.get("evidence_type") == "image"
        )
        mapping_by_id = {int(item["id"]): item for item in ai_mappings}
        attributes: list[dict] = []
        complex_attributes: list[dict] = []
        suggestions: dict[int, tuple[str, str]] = {}
        dictionary_requests: list[dict] = []
        dictionary_options_by_attribute: dict[int, dict[int, dict]] = {}
        automatic_count = 0
        exact_dictionary_count = 0

        for definition in definitions:
            attribute_id = int(definition["id"])
            name = str(definition.get("name") or attribute_id)
            name_ru = str(definition.get("name_ru") or "")
            default_kind = self._system_default_attribute_kind(definition)
            if default_kind:
                options = []
                if default_kind in {"warranty", "country_of_origin"}:
                    options, _, _ = self._cached_dictionary_values(
                        api, category_id, type_id, attribute_id,
                    )
                self._apply_system_attribute_default(
                    attributes, complex_attributes, definition,
                    dictionary_options=options,
                )
                if default_kind in {"warranty", "country_of_origin"}:
                    exact_dictionary_count += 1
                else:
                    automatic_count += 1
                continue
            if self._is_product_weight_attribute(definition):
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=self.vars["net_weight"].get().strip(), source="自动工作台：商品净重",
                )
                automatic_count += 1
                continue
            if self._is_model_name_attribute(definition):
                if int(definition.get("dictionary_id") or 0):
                    raise ValueError("Ozon 将型号名称返回成了字典属性，无法写入人工型号名称")
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=self.vars["model_name"].get().strip(), source="人工填写：型号名称（合并多变体）",
                )
                automatic_count += 1
                continue
            mapping = mapping_by_id.get(attribute_id)
            kind = self._content_attribute_kind(name) or self._content_attribute_kind(name_ru)
            direct_value = self._content_value(kind) if kind else ""
            direct_source = "生成文案" if direct_value else ""
            if not direct_value:
                fact = find_reference_fact(name_ru or name, self.reference)
                if fact:
                    direct_source, direct_value = fact
            mapped_values = list(mapping.get("values", [])) if mapping else []
            candidate_values = []
            if direct_value:
                candidate_values.append(direct_value)
            candidate_values.extend(mapped_values)
            candidate_values = list(dict.fromkeys(
                re.sub(r"\s+", " ", str(value)).strip()
                for value in candidate_values if str(value).strip()
            ))
            if mapping:
                if mapping.get("evidence_type") == "image":
                    confidence = mapping.get("confidence")
                    confidence_text = f"，置信度 {float(confidence):.0%}" if confidence is not None else ""
                    ai_source = f"AI图片证据{confidence_text}"
                else:
                    ai_source = "AI推断" if mapping.get("inferred") else "AI映射页面事实"
                if mapping.get("evidence"):
                    ai_source += "：" + mapping["evidence"]
            else:
                ai_source = direct_source

            if int(definition.get("dictionary_id") or 0) and self._is_brand_attribute(definition):
                no_brand = self._locked_no_brand_dictionary_value()
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=str(no_brand.get("value") or "Нет бренда"),
                    dictionary_value_id=int(no_brand["id"]), source="系统锁定：无品牌",
                )
                exact_dictionary_count += 1
                continue

            if not candidate_values:
                continue

            if not int(definition.get("dictionary_id") or 0):
                values_to_write = candidate_values if definition.get("is_collection") else candidate_values[:1]
                for index, value in enumerate(values_to_write):
                    set_attribute_value(
                        attributes, complex_attributes, definition, value=value,
                        source=direct_source or ai_source,
                        append=index > 0,
                    )
                automatic_count += 1
                continue

            options: dict[int, dict] = {}
            unresolved_values = []
            exact_found = False
            results, _, _ = self._cached_dictionary_values(api, category_id, type_id, attribute_id)
            for candidate in candidate_values:
                if len(candidate) < 2:
                    unresolved_values.append(candidate)
                    continue
                exact = self._exact_dictionary_value(results, candidate)
                if exact:
                    set_attribute_value(
                        attributes, complex_attributes, definition,
                        value=str(exact.get("value") or candidate), dictionary_value_id=int(exact["id"]),
                        source=f"{direct_source or ai_source} + Ozon字典精确匹配",
                        append=bool(definition.get("is_collection")),
                    )
                    exact_dictionary_count += 1
                    exact_found = True
                    if not definition.get("is_collection"):
                        break
                else:
                    unresolved_values.append(candidate)
            for option in self._dictionary_options_for_ai(
                results, unresolved_values or candidate_values,
            ):
                if option.get("id") and str(option.get("value") or "").strip():
                    options[int(option["id"])] = {
                        "id": int(option["id"]), "value": str(option["value"]),
                    }
            if options and (definition.get("is_collection") or not exact_found):
                maximum = int(definition.get("max_value_count") or 0)
                dictionary_options_by_attribute[attribute_id] = options
                dictionary_requests.append({
                    "attribute_id": attribute_id,
                    "attribute_name": name,
                    "attribute_name_ru": name_ru,
                    "attribute_description": re.sub(
                        r"\s+", " ", str(definition.get("description") or "")
                    ).strip()[:500],
                    "source_values": [
                        str(value)[:240]
                        for value in (unresolved_values or candidate_values)[:10]
                    ],
                    "source_evidence": str(direct_source or ai_source)[:400],
                    "collection": bool(definition.get("is_collection")),
                    "max_value_count": maximum,
                    "options": list(options.values()),
                })
            elif not exact_found:
                suggestions[attribute_id] = (direct_source or ai_source, "; ".join(candidate_values))

        ai_dictionary_count = 0
        if dictionary_requests:
            option_count = sum(len(request["options"]) for request in dictionary_requests)
            self._log(
                f"AI 正在从 {len(dictionary_requests)} 个属性的 {option_count} 个真实 Ozon "
                "字典选项中选择有效 ID……"
            )
            chosen_ids = ai.choose_dictionary_values(dictionary_requests)
            for attribute_id, value_ids in chosen_ids.items():
                definition = next((item for item in definitions if int(item["id"]) == attribute_id), None)
                if not definition:
                    continue
                options = dictionary_options_by_attribute.get(attribute_id, {})
                for index, value_id in enumerate(value_ids):
                    option = options.get(int(value_id))
                    if not option:
                        continue
                    set_attribute_value(
                        attributes, complex_attributes, definition,
                        value=option["value"], dictionary_value_id=option["id"],
                        source="AI选择 + Ozon实时字典校验",
                        append=bool(definition.get("is_collection")) and index >= 0,
                    )
                    ai_dictionary_count += 1
                if value_ids:
                    suggestions.pop(attribute_id, None)
            selected_attribute_ids = set(chosen_ids)
            for request in dictionary_requests:
                attribute_id = int(request["attribute_id"])
                if attribute_id not in selected_attribute_ids and attribute_id not in attribute_values_by_id(attributes, complex_attributes):
                    suggestions[attribute_id] = ("AI未找到等义字典值", "; ".join(request["source_values"]))

        unique_type_count = self._enforce_unique_required_product_type(
            attributes, complex_attributes, api=api,
            category_id=category_id, type_id=type_id, definitions=definitions,
        )
        if unique_type_count:
            automatic_count += unique_type_count
            for definition in definitions:
                if self._is_product_type_attribute(definition):
                    suggestions.pop(int(definition["id"]), None)

        missing = validate_required(attributes, definitions, complex_attributes)
        category_by_display = {item["display"]: item for item in categories}
        displays = list(category_by_display)
        reason = selected_category.get("ai_reason") or ""

        def update():
            self.categories = categories
            self.category_displays = displays
            self.category_by_display = category_by_display
            self.category_combo.configure(values=displays)
            self.category_combo.set(selected_category["display"])
            self.vars["category_id"].set(str(category_id))
            self.vars["type_id"].set(str(type_id))
            self.attribute_definitions = definitions
            self.attribute_definition_by_id = {int(item["id"]): item for item in definitions}
            self.attribute_fact_suggestions = suggestions
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()
        self.root.after(0, update)
        self._log(
            f"AI 自动完成：类目 1 个、普通属性 {automatic_count} 项、字典精确匹配 {exact_dictionary_count} 个、"
            f"AI 从 Ozon 字典选值 {ai_dictionary_count} 个、采用图片证据 {image_mapping_count} 项；"
            f"仍缺必填 {len(missing)} 项"
        )
        if keywords:
            self._log("AI 商品类型关键词：" + "、".join(keywords))
        if reason:
            self._log("AI 类目依据：" + reason)
        if missing:
            self._log("页面无充分依据或 Ozon 字典无等义值，程序不会伪造以下必填项：" + "、".join(missing))

    def _load_attributes(self):
        category_id = int(self.vars["category_id"].get())
        type_id = int(self.vars["type_id"].get())
        api = self._api()
        definitions = self._localized_attributes(api, category_id, type_id)
        if not definitions:
            raise RuntimeError("Ozon 没有返回该类目/类型的属性，请重新确认类目")
        if not any(self._is_model_name_attribute(item) for item in definitions):
            raise ValueError(
                "当前 Ozon 实时类目没有“型号名称（针对合并为一张商品卡片）”属性，"
                "请检查类目是否支持合并多变体"
            )
        required = [item for item in definitions if item.get("is_required")]
        attributes: list[dict] = []
        complex_attributes: list[dict] = []
        suggestions: dict[int, tuple[str, str]] = {}
        filled_from_page = 0
        filled_from_content = 0
        filled_dictionary = 0
        for definition in definitions:
            name = str(definition.get("name") or "")
            name_ru = str(definition.get("name_ru") or "")
            attribute_id = int(definition["id"])
            default_kind = self._system_default_attribute_kind(definition)
            if default_kind:
                options = []
                if default_kind in {"warranty", "country_of_origin"}:
                    options, _, _ = self._cached_dictionary_values(
                        api, category_id, type_id, attribute_id,
                    )
                self._apply_system_attribute_default(
                    attributes, complex_attributes, definition,
                    dictionary_options=options,
                )
                if default_kind in {"warranty", "country_of_origin"}:
                    filled_dictionary += 1
                else:
                    filled_from_content += 1
                continue
            if self._is_product_weight_attribute(definition):
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=self.vars["net_weight"].get().strip(), source="自动工作台：商品净重",
                )
                filled_from_page += 1
                continue
            if self._is_model_name_attribute(definition):
                if int(definition.get("dictionary_id") or 0):
                    raise ValueError("Ozon 将型号名称返回成了字典属性，无法写入人工型号名称")
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=self.vars["model_name"].get().strip(), source="人工填写：型号名称（合并多变体）",
                )
                filled_from_content += 1
                continue
            if int(definition.get("dictionary_id") or 0) and self._is_brand_attribute(definition):
                no_brand = self._locked_no_brand_dictionary_value()
                set_attribute_value(
                    attributes, complex_attributes, definition,
                    value=str(no_brand["value"]), dictionary_value_id=int(no_brand["id"]),
                    source="系统锁定：无品牌",
                )
                filled_dictionary += 1
                continue
            kind = self._content_attribute_kind(name) or self._content_attribute_kind(name_ru)
            candidate = self._content_value(kind) if kind else ""
            source_name = "生成文案（待核对）" if candidate else ""
            if not candidate:
                fact = find_reference_fact(name_ru or name, self.reference)
                if fact:
                    source_name, candidate = fact
            if not candidate:
                continue
            if int(definition.get("dictionary_id") or 0):
                if len(candidate.strip()) < 2:
                    suggestions[attribute_id] = (source_name, candidate)
                    continue
                results, _, _ = self._cached_dictionary_values(api, category_id, type_id, attribute_id)
                exact = self._exact_dictionary_value(results, candidate)
                if exact:
                    set_attribute_value(
                        attributes, complex_attributes, definition,
                        value=str(exact.get("value") or candidate), dictionary_value_id=int(exact["id"]),
                        source=f"{source_name} + Ozon 字典精确匹配",
                    )
                    filled_dictionary += 1
                else:
                    suggestions[attribute_id] = (source_name, candidate)
                continue
            set_attribute_value(
                attributes, complex_attributes, definition,
                value=candidate, source=source_name,
            )
            if kind:
                filled_from_content += 1
            else:
                filled_from_page += 1

        filled_dictionary += self._enforce_unique_required_product_type(
            attributes, complex_attributes, api=api,
            category_id=category_id, type_id=type_id, definitions=definitions,
        )

        def update():
            self.attribute_definitions = definitions
            self.attribute_definition_by_id = {int(item["id"]): item for item in definitions}
            self.attribute_fact_suggestions = suggestions
            self._write_attribute_documents(attributes, complex_attributes)
            self._refresh_attribute_tree()
        self.root.after(0, update)
        missing = validate_required(attributes, definitions, complex_attributes)
        self._log(
            f"读取实时属性 {len(definitions)} 项（必填 {len(required)}）；自动填写页面事实 {filled_from_page} 项、"
            f"文案字段 {filled_from_content} 项、Ozon 字典精确匹配 {filled_dictionary} 项；仍缺必填 {len(missing)} 项"
        )
        if suggestions:
            self._log(f"另有 {len(suggestions)} 个页面候选值无法与 Ozon 字典精确匹配，已标为待人工选择，未自动提交")

    def _load_warehouses(self):
        shop = self._ozon_shop_for_api()
        warehouses = self._api(shop.get("id") or None).warehouses()
        mapping = {}
        for item in warehouses:
            warehouse_id = item.get("warehouse_id") or item.get("id")
            name = str(item.get("name") or item.get("warehouse_name") or warehouse_id)
            if item.get("is_rfbs") is True:
                scheme = "rFBS"
            elif item.get("is_rfbs") is False:
                scheme = "FBS"
            else:
                scheme = str(item.get("type") or item.get("scheme") or item.get("integration_type") or "未标识模式")
            status = str(item.get("status") or "")
            display = f"{name} | {scheme} | {status} | ID={warehouse_id}"
            mapping[display] = item
        self.warehouse_by_display = mapping
        self.root.after(0, lambda: self.warehouse_combo.configure(values=list(mapping)))
        rfbs_count = sum(1 for item in warehouses if item.get("is_rfbs") is True)
        self._log(
            f"店铺“{shop.get('name') or '未命名店铺'}”读取到 {len(mapping)} 个仓库，其中 rFBS {rfbs_count} 个；"
            "请选择卖家后台已启用的 rFBS 仓库"
        )

    def _warehouse_selected(self):
        item = self.warehouse_by_display.get(self.warehouse_combo.get(), {})
        warehouse_id = item.get("warehouse_id") or item.get("id")
        if warehouse_id:
            self.vars["warehouse_id"].set(str(warehouse_id))
            current_shop_id = self.vars.get("ozon_shop_id").get().strip() if self.vars.get("ozon_shop_id") else ""
            work_shop_id = self.job_form_vars.get("ozon_shop_id").get().strip() if self.job_form_vars.get("ozon_shop_id") else ""
            if current_shop_id and current_shop_id == work_shop_id:
                self._job_var("warehouse_id").set(str(warehouse_id))

    def _generated_content(self):
        formatted_tags = format_ozon_hashtags(self.vars["tags"].get())
        return {
            "title_ru": self.vars["title"].get().strip(),
            "description_ru": self.description_text.get("1.0", tk.END).strip(),
            "tags_ru": parse_ozon_hashtags(formatted_tags),
        }

    def _draft_data(self):
        content = self._generated_content()
        title = content["title_ru"]
        if not title:
            raise ValueError("商品标题不能为空")
        content["tags_ru"] = validate_ozon_hashtags(content["tags_ru"])
        listing_reference = ReferenceProduct(
            self.reference.source_url or self.vars["source_url"].get().strip(), title, content["description_ru"],
            list(self.reference.images), dict(self.reference.properties), list(self.reference.tags),
        )
        attributes = parse_attributes(self.attribute_text.get("1.0", tk.END))
        complex_attributes = parse_complex_attributes(self.complex_attribute_text.get("1.0", tk.END))
        self._enforce_locked_brand(attributes, complex_attributes)
        self._enforce_manual_net_weight(attributes, complex_attributes)
        self._enforce_manual_model_name(attributes, complex_attributes)
        self._enforce_system_defaults_before_submission(attributes, complex_attributes)
        self._enforce_generated_content_attributes(attributes, complex_attributes, content)
        unique_type_count = self._enforce_unique_required_product_type(
            attributes, complex_attributes,
        )
        if unique_type_count:
            self._log("提交前已自动填写 Ozon 当前类目唯一可选的必填“类型”")
        repaired_dictionary_values = self._validate_dictionary_values_before_submission(
            attributes, complex_attributes,
        )
        if repaired_dictionary_values:
            self._log(
                f"提交前已将 {repaired_dictionary_values} 个字典属性值纠正为 Ozon 当前可选项及真实 ID"
            )
        invalid_chinese = unsupported_chinese_submission_fields(
            title, content["description_ru"], content["tags_ru"], attributes,
            complex_attributes, self.attribute_definitions,
        )
        if invalid_chinese:
            raise ValueError(
                "以下自由填写字段包含中文，Ozon 只允许俄文：" + "、".join(invalid_chinese)
                + "。字典下拉属性可以保留 Ozon 返回的中文值和 dictionary_value_id。"
            )
        attributes = clean_attribute_payload(attributes)
        complex_attributes = clean_attribute_payload(complex_attributes)
        product = ProductInput(
            self.vars["price"].get(), float(self.vars["weight"].get()), float(self.vars["depth"].get()),
            float(self.vars["width"].get()), float(self.vars["height"].get()), int(self.vars["category_id"].get()),
            int(self.vars["type_id"].get()), self.vars["offer_id"].get(), self.vars["currency_code"].get(),
            self.vars["weight_unit"].get(), self.vars["dimension_unit"].get(), self.vars["vat"].get(),
            old_price=self.vars["old_price"].get(),
        )
        item = build_import_item(product, listing_reference, self.uploaded_urls, attributes, complex_attributes)
        return item, attributes, complex_attributes

    def _save_draft(self, notify=True):
        item, _, _ = self._draft_data()
        output = atomic_write_json(OUTPUT_DIR / f"draft-{item['offer_id']}.json", {
            "source": export_reference(self.reference), "content": self._generated_content(),
            "item": item, "submitted": False,
        })
        self._log("草稿已保存：" + str(output))
        if notify:
            self.root.after(0, lambda: messagebox.showinfo("草稿完成", f"未提交 Ozon。\n{output}"))
        return output

    @staticmethod
    def _adjusted_old_price(old_price, new_price: Decimal) -> str:
        try:
            current_old_price = Decimal(str(old_price or "0"))
        except (InvalidOperation, ValueError):
            current_old_price = Decimal("0")
        if current_old_price <= 0:
            return "0"
        minimum_old_price = (
            new_price * Decimal("1.25")
        ).to_integral_value(rounding=ROUND_CEILING)
        if current_old_price >= minimum_old_price:
            return format(current_old_price, "f")
        return format(minimum_old_price, "f")

    def _commission_pricing_values(self, job_record=None) -> tuple[dict[str, str], bool]:
        inputs = (job_record or {}).get("inputs") if isinstance((job_record or {}).get("inputs"), dict) else {}
        values: dict[str, str] = {}
        for key in (
            "purchase_cost", "label_fee", "target_roi", "advertising_percent",
            "cargo_loss_percent", "sales_commission_percent", "weight", "fbp_pricing",
        ):
            value = str(inputs.get(key) or "").strip()
            if not value and job_record is None and key in self.vars:
                value = self.vars[key].get().strip()
            values[key] = value
        values["advertising_percent"] = values["advertising_percent"] or "15"
        values["cargo_loss_percent"] = values["cargo_loss_percent"] or "10"
        try:
            manual_commission = Decimal(values["sales_commission_percent"])
        except (InvalidOperation, ValueError) as error:
            raise ValueError("请填写有效的 Ozon 标准佣金百分比") from error
        if not manual_commission.is_finite() or manual_commission <= 0 or manual_commission >= 100:
            raise ValueError("Ozon 标准佣金必须大于 0% 且小于 100%")
        if self._fbp_pricing_enabled(values):
            if manual_commission <= 1:
                raise ValueError("FBP 核价要求填写的标准佣金大于 1%")
            values["label_fee"] = "0"
        pricing_fields = ("purchase_cost", "label_fee", "target_roi")
        present = [bool(values[key]) for key in pricing_fields]
        if any(present) and not all(present):
            raise ValueError("按填写佣金核价需要完整填写采购成本、贴单费和目标 ROI")
        if all(present) and not values["weight"]:
            raise ValueError("按填写佣金核价需要填写包装毛重")
        return values, all(present)

    def _record_manual_commission_pricing(
        self, item: dict, info: dict, pricing_result: dict, job_record=None,
    ):
        item["price"] = str(info["price"])
        try:
            actual_old_price = Decimal(str(info.get("old_price") or "0"))
        except (InvalidOperation, ValueError):
            actual_old_price = Decimal("0")
        item["old_price"] = (
            format(actual_old_price, "f")
            if actual_old_price > Decimal(str(info["price"])) else "0"
        )
        item["currency_code"] = str(info.get("currency_code") or item.get("currency_code") or "RUB")

        def update_workspace_price():
            if "price" in self.vars:
                self.vars["price"].set(item["price"])
            if "old_price" in self.vars:
                self.vars["old_price"].set(item["old_price"])

        self._ui_call(update_workspace_price)
        if job_record is not None:
            inputs = job_record.get("inputs") if isinstance(job_record.get("inputs"), dict) else {}
            inputs["price"] = item["price"]
            inputs["old_price"] = item["old_price"]
            inputs["currency_code"] = item["currency_code"]
            job_record["inputs"] = inputs
            job_record.pop("commission_verified", None)
            job_record.pop("ozon_sales_commission_percent", None)
            job_record["manual_commission_calculated"] = True
            job_record["actual_sales_commission_percent"] = pricing_result["sales_percent_rfbs"]
            job_record["manual_sales_commission_percent"] = pricing_result["manual_sales_commission_percent"]
            job_record["commission_pricing"] = pricing_result
            pricing_mode = pricing_result.get("pricing_mode") or "RFBS"
            job_record["message"] = (
                f"{pricing_mode} 按填写佣金核价：标准 {pricing_result['manual_sales_commission_percent']}%，"
                f"实际计算 {pricing_result['sales_percent_rfbs']}%，售价 {item['price']}"
            )
            self._save_auto_jobs()

    def _apply_manual_commission_price(self, api, item: dict, job_record=None, cancel_check=None) -> dict:
        values, pricing_enabled = self._commission_pricing_values(job_record)
        fbp_pricing = self._fbp_pricing_enabled(values)
        pricing_mode = "FBP" if fbp_pricing else "RFBS"
        commission_discount = Decimal("1") if fbp_pricing else Decimal("0")
        manual_commission_percent = Decimal(values["sales_commission_percent"])
        effective_commission_percent = manual_commission_percent - commission_discount
        if effective_commission_percent <= 0:
            raise ValueError("FBP 减免后的销售佣金必须大于 0%")
        info = {
            "price": str(item["price"]),
            "old_price": str(item.get("old_price") or "0"),
            "currency_code": str(item.get("currency_code") or "RUB"),
        }
        category_id = int(item["description_category_id"])
        type_id = int(item["type_id"])
        base_result = {
            "verified": True,
            "commission_source": "manual",
            "pricing_enabled": pricing_enabled,
            "pricing_mode": pricing_mode,
            "commission_discount_percent": format(commission_discount, "f"),
            "description_category_id": category_id,
            "type_id": type_id,
            "manual_sales_commission_percent": format(manual_commission_percent, "f"),
            "sales_percent_rfbs": format(effective_commission_percent, "f"),
            "price": str(info["price"]),
            "adjustments": 0,
        }
        if not pricing_enabled:
            self._log(
                f"Ozon {pricing_mode} 使用人工填写的标准佣金 {format(manual_commission_percent, 'f')}%，"
                f"实际计算佣金 {format(effective_commission_percent, 'f')}%；"
                "未填写 ROI 成本参数，因此保留当前售价"
            )
            self._record_manual_commission_pricing(item, info, base_result, job_record)
            return base_result

        weight_kg = Decimal(values["weight"]) / Decimal("1000")
        target_roi = Decimal(values["target_roi"])
        if cancel_check:
            cancel_check()
        current_price = Decimal(str(info["price"]))
        current = calculate_price_breakdown(
            current_price, values["purchase_cost"], values["label_fee"],
            weight_kg, manual_commission_percent,
            sales_commission_discount_percent=commission_discount,
            advertising_percent=values["advertising_percent"],
            cargo_loss_percent=values["cargo_loss_percent"],
        )
        adjustment_count = 0
        if current.roi_percent < target_roi:
            minimum_update_price = (
                current_price * Decimal("1.05")
            ).to_integral_value(rounding=ROUND_CEILING)
            required = calculate_roi_price(
                values["purchase_cost"], values["label_fee"], values["target_roi"],
                weight_kg, sales_commission_percent=manual_commission_percent,
                sales_commission_discount_percent=commission_discount,
                advertising_percent=values["advertising_percent"],
                cargo_loss_percent=values["cargo_loss_percent"],
                minimum_sale_price=minimum_update_price,
            )
            new_price = required.price
            new_old_price = self._adjusted_old_price(
                info.get("old_price") or item.get("old_price"), new_price,
            )
            currency_code = str(info.get("currency_code") or item.get("currency_code") or "RUB")
            self._log(
                f"当前售价未达到目标 ROI，按填写佣金将 Ozon 售价从 {format(current_price, 'f')} 调整为 "
                f"{format(new_price, 'f')} {currency_code}"
            )
            api.update_price(
                item["offer_id"], format(new_price, "f"), old_price=new_old_price,
                currency_code=currency_code, min_price="0",
            )
            info.update({
                "price": format(new_price, "f"),
                "old_price": new_old_price,
                "currency_code": currency_code,
            })
            current_price = new_price
            current = calculate_price_breakdown(
                current_price, values["purchase_cost"], values["label_fee"],
                weight_kg, manual_commission_percent,
                sales_commission_discount_percent=commission_discount,
                advertising_percent=values["advertising_percent"],
                cargo_loss_percent=values["cargo_loss_percent"],
            )
            adjustment_count = 1

        result = {
            **base_result,
            "price": format(current_price, "f"),
            "target_roi_percent": format(target_roi, "f"),
            "actual_roi_percent": self._pricing_amount(current.roi_percent),
            "shipping": self._pricing_amount(current.shipping),
            "sales_commission": self._pricing_amount(current.sales_commission),
            "logistics_commission": self._pricing_amount(current.logistics_commission),
            "advertising_percent": values["advertising_percent"],
            "cargo_loss_percent": values["cargo_loss_percent"],
            "advertising": self._pricing_amount(current.advertising),
            "cargo_loss": self._pricing_amount(current.cargo_loss),
            "profit": self._pricing_amount(current.profit),
            "adjustments": adjustment_count,
        }
        self._record_manual_commission_pricing(item, info, result, job_record)
        self._log(
            f"Ozon {pricing_mode} 人工佣金核价完成：标准佣金 "
            f"{result['manual_sales_commission_percent']}%，实际计算 {result['sales_percent_rfbs']}%，"
            f"售价 {item['price']} {item['currency_code']}，ROI {result['actual_roi_percent']}%"
        )
        return result

    def _product_ledger_record(
        self, item: dict, pricing_result: dict, task_id: int,
        warehouse_id: int, stock: int, job_record=None,
    ) -> dict:
        inputs = (
            dict(job_record.get("inputs") or {})
            if isinstance((job_record or {}).get("inputs"), dict) else {}
        )

        def input_value(key: str, default=""):
            if key in inputs:
                return inputs.get(key)
            if job_record is not None:
                return default
            variable = getattr(self, "vars", {}).get(key)
            return variable.get().strip() if variable is not None else default

        shop = self._ozon_shop_for_api(str(inputs.get("ozon_shop_id") or "") or None)
        fbp_pricing = self._fbp_pricing_enabled({"fbp_pricing": input_value("fbp_pricing")})
        local_listing = self._local_listing_enabled({
            "listing_mode": input_value("listing_mode", "follow"),
        })
        return {
            "platform": "Ozon",
            "shop_name": inputs.get("ozon_shop_name") or shop.get("name") or "",
            "shop_config_id": inputs.get("ozon_shop_id") or shop.get("id") or "",
            "listing_mode": "本地新品" if local_listing else "跟卖",
            "offer_id": item.get("offer_id"),
            "supplier_1688_url": input_value("supplier_1688_url"),
            "ozon_source_url": input_value("source_url"),
            "ozon_product_id": pricing_result.get("product_id"),
            "title": item.get("name"),
            "model_name": input_value("model_name"),
            "price": item.get("price"),
            "old_price": item.get("old_price"),
            "currency_code": item.get("currency_code"),
            "pricing_mode": pricing_result.get("pricing_mode") or ("FBP" if fbp_pricing else "RFBS"),
            "purchase_cost": input_value("purchase_cost"),
            "label_fee": "0" if fbp_pricing else input_value("label_fee"),
            "target_roi_percent": input_value("target_roi"),
            "actual_roi_percent": pricing_result.get("actual_roi_percent"),
            "advertising_percent": input_value("advertising_percent", "15") or "15",
            "cargo_loss_percent": input_value("cargo_loss_percent", "10") or "10",
            "raw_sales_commission_percent": (
                pricing_result.get("manual_sales_commission_percent")
                or input_value("sales_commission_percent")
            ),
            "sales_commission_percent": pricing_result.get("sales_percent_rfbs"),
            "shipping": pricing_result.get("shipping"),
            "sales_commission": pricing_result.get("sales_commission"),
            "logistics_commission": pricing_result.get("logistics_commission"),
            "advertising": pricing_result.get("advertising"),
            "cargo_loss": pricing_result.get("cargo_loss"),
            "profit": pricing_result.get("profit"),
            "net_weight_g": input_value("net_weight"),
            "gross_weight_g": item.get("weight"),
            "depth_mm": item.get("depth"),
            "width_mm": item.get("width"),
            "height_mm": item.get("height"),
            "category_display": input_value("category_display"),
            "category_id": item.get("description_category_id"),
            "type_id": item.get("type_id"),
            "warehouse_id": warehouse_id,
            "stock": stock,
            "import_task_id": task_id,
            "status": "上架及 RFBS 库存完成",
        }

    def _confirm_submit(self):
        try:
            shop = self._ozon_shop_for_api()
        except Exception as error:
            messagebox.showerror("未选择上品店铺", str(error))
            return
        warehouse_id = self.vars["warehouse_id"].get().strip()
        if not messagebox.askyesno(
            "确认真实提交",
            f"店铺：{shop.get('name') or '未命名店铺'}\nClient-Id：{shop.get('client_id')}\n"
            f"RFBS 仓库 ID：{warehouse_id or '未填写'}\n\n"
            "这会创建 Ozon 商品并写入 RFBS 库存。是否已经人工检查草稿、类目、属性、图片、价格和尺寸？",
        ):
            return
        self._run(self._submit)

    def _submit(self, notify=True, auto_progress=False, job_record=None):
        cancel_check = (
            (lambda: self._check_auto_job_cancelled(job_record))
            if job_record is not None else None
        )
        if cancel_check:
            cancel_check()
        item, attributes, complex_attributes = self._draft_data()
        api = self._api()
        definitions = self._retry_step(
            "Ozon 必填属性校验",
            lambda: api.attributes(item["description_category_id"], item["type_id"]),
            attempts=3,
            cancel_check=cancel_check,
        )
        missing = validate_required(attributes, definitions, complex_attributes)
        if missing:
            raise RuntimeError("仍缺少 Ozon 必填属性：" + "、".join(missing))
        task_id = int((job_record or {}).get("task_id") or 0)
        if not task_id:
            if cancel_check:
                cancel_check()
            task_id = api.import_product(item)
            if job_record is not None:
                job_record["task_id"] = task_id
                self._save_auto_jobs()
            self._log(f"Ozon 已接收导入任务：{task_id}")
        else:
            self._log(f"继续查询已有 Ozon 导入任务：{task_id}；不会重复发送商品创建请求")
        if auto_progress:
            self._set_auto_progress(8, f"8/9 Ozon 已接收任务 {task_id}，正在按填写佣金核验售价")
        if cancel_check:
            cancel_check()
        result = api.wait_import(
            task_id, log_func=self._log, cancel_check=cancel_check,
        )
        result_items = (result or {}).get("result", {}).get("items", [])
        errors = [entry for entry in result_items if entry.get("errors") or str(entry.get("status", "")).casefold() in {"failed", "error"}]
        if errors:
            raise RuntimeError("商品导入失败，未写入库存：" + json.dumps(errors, ensure_ascii=False))
        statuses = {str(entry.get("status") or "").casefold() for entry in result_items}
        if "imported" not in statuses:
            raise RuntimeError(
                f"Ozon 导入任务 {task_id} 尚未明确完成，程序不会提前写入库存。"
                "请稍后按任务 ID 或 Offer ID 查询后再处理。"
            )
        if job_record is not None:
            job_record["import_completed"] = True
            self._save_auto_jobs()
        if cancel_check:
            cancel_check()
        pricing_result = self._apply_manual_commission_price(
            api, item, job_record=job_record, cancel_check=cancel_check,
        )
        if cancel_check:
            cancel_check()
        warehouse_id = int(self.vars["warehouse_id"].get())
        stock = int(self.vars["stock"].get())
        if stock < 0:
            raise ValueError("库存不能小于 0")
        if job_record is not None and job_record.get("stock_completed"):
            stock_result = {"resumed": True, "message": "库存此前已写入"}
        else:
            stock_result = self._retry_step(
                "RFBS 库存写入",
                lambda: api.update_stock(item["offer_id"], warehouse_id, stock),
                attempts=3,
                cancel_check=cancel_check,
            )
            if job_record is not None:
                job_record["stock_completed"] = True
                self._save_auto_jobs()
        ledger_result = upsert_product_ledger(
            PRODUCT_LEDGER_PATH,
            self._product_ledger_record(
                item, pricing_result, task_id, warehouse_id, stock,
                job_record=job_record,
            ),
        )
        if job_record is not None:
            job_record["ledger_completed"] = True
            job_record["ledger_path"] = ledger_result["path"]
            self._save_auto_jobs()
        action_name = {
            "created": "新建并写入", "added": "追加", "updated": "更新",
        }.get(ledger_result["action"], "写入")
        self._log(
            f"产品台账已{action_name}：{ledger_result['path']}（第 {ledger_result['row']} 行）"
        )
        output = atomic_write_json(OUTPUT_DIR / f"submitted-{item['offer_id']}.json", {
            "source": export_reference(self.reference), "content": self._generated_content(),
            "item": item, "submitted": True,
            "task_id": task_id, "import_result": result,
            "commission_pricing": pricing_result, "stock_result": stock_result,
            "product_ledger": ledger_result,
        })
        self._log("提交记录已保存：" + str(output))
        if notify:
            self.root.after(0, lambda: messagebox.showinfo(
                "提交完成",
                f"导入任务 {task_id} 已完成，RFBS 库存和产品台账均已更新。\n"
                f"台账：{ledger_result['path']}\n请继续在卖家后台检查审核和可售状态。",
            ))
        return output


def launch():
    root = tk.Tk()
    RfbsListingApp(root)
    root.mainloop()
