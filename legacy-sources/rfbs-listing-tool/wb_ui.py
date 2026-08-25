from __future__ import annotations

import json
import re
import shutil
import threading
import time
import uuid
import tkinter as tk
from decimal import Decimal, InvalidOperation
from pathlib import Path
from tkinter import filedialog, messagebox, ttk

from core import atomic_write_json, load_json
from product_ledger import upsert_product_ledger
from services import WbCardAiService
from wb import (
    WildberriesApi, WbListingWorkflow, build_card_content_document,
    build_card_payload, parse_card_content_document, validate_image_paths,
)
from wb_pricing import WbPricingBreakdown, calculate_wb_price
from wb_source import (
    download_wb_source_images,
    normalize_wb_source,
    scrape_wb_source,
)


class WbUiMixin:
    """Wildberries UI and resumable listing workflow for ``RfbsListingApp``."""

    def _initialize_wb_state(self):
        self.wb_parents: list[dict] = []
        self.wb_parent_by_display: dict[str, dict] = {}
        self.wb_subjects: list[dict] = []
        self.wb_subject_by_display: dict[str, dict] = {}
        self.wb_characteristics: list[dict] = []
        self.wb_characteristic_values: dict[str, object] = {}
        self.wb_directory_by_display: dict[str, object] = {}
        self.wb_image_paths: list[str] = []
        self.wb_warehouse_by_display: dict[str, dict] = {}
        self.wb_source_reference: dict = {}
        self.wb_job: dict = {}
        self.wb_pricing_breakdown: WbPricingBreakdown | None = None
        self._wb_pricing_trace_suspended = False

    def _build_wb(self):
        tab = self.wb_tab
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(1, weight=1)

        connection = ttk.LabelFrame(tab, text="WB API", padding=8)
        connection.grid(row=0, column=0, sticky="ew", pady=(0, 8))
        connection.columnconfigure(1, weight=1)
        ttk.Label(connection, text="个人 Token").grid(row=0, column=0, sticky="w", padx=4)
        ttk.Entry(
            connection, textvariable=self._var("wb_token"), show="*",
        ).grid(row=0, column=1, sticky="ew", padx=4)
        ttk.Checkbutton(
            connection, text="Sandbox", variable=self._var("wb_sandbox", "0"),
            onvalue="1", offvalue="0", command=self._refresh_wb_config_state,
        ).grid(row=0, column=2, padx=5)
        ttk.Button(
            connection, text="保存", command=self._save_wb_config,
        ).grid(row=0, column=3, padx=4)
        ttk.Button(
            connection, text="测试权限", command=lambda: self._run(self._test_wb_connection),
        ).grid(row=0, column=4, padx=4)
        self.wb_connection_var = tk.StringVar(value="尚未配置 WB API Token")
        ttk.Label(
            connection, textvariable=self.wb_connection_var, foreground="#586570",
        ).grid(row=1, column=0, columnspan=5, sticky="w", padx=4, pady=(6, 0))

        self.wb_product_tabs = ttk.Notebook(tab)
        self.wb_product_tabs.grid(row=1, column=0, sticky="nsew")
        product_tab = ttk.Frame(self.wb_product_tabs, padding=10)
        category_tab = ttk.Frame(self.wb_product_tabs, padding=10)
        media_tab = ttk.Frame(self.wb_product_tabs, padding=10)
        self.wb_product_tabs.add(product_tab, text="1. 商品信息")
        self.wb_product_tabs.add(category_tab, text="2. 类目与属性")
        self.wb_product_tabs.add(media_tab, text="3. 图片与库存")
        self._build_wb_product_tab(product_tab)
        self._build_wb_category_tab(category_tab)
        self._build_wb_media_tab(media_tab)
        self._wb_on_listing_mode_changed()
        self._wb_on_fulfillment_mode_changed()

        status = ttk.LabelFrame(tab, text="WB 上架任务", padding=8)
        status.grid(row=2, column=0, sticky="ew", pady=(8, 0))
        status.columnconfigure(1, weight=1)
        self.wb_submit_button = ttk.Button(
            status, text="开始上架", command=self._confirm_wb_submit, state="disabled",
        )
        self.wb_submit_button.grid(row=0, column=0, rowspan=2, sticky="ns", padx=(0, 10))
        self.wb_progress = ttk.Progressbar(status, maximum=6, mode="determinate")
        self.wb_progress.grid(row=0, column=1, sticky="ew")
        self.wb_status_var = tk.StringVar(value="API 尚未配置")
        ttk.Label(status, textvariable=self.wb_status_var, foreground="#245a8d").grid(
            row=1, column=1, sticky="w", pady=(5, 0),
        )
        self.wb_detail_var = tk.StringVar(value="")
        ttk.Label(status, textvariable=self.wb_detail_var, foreground="#8a5a00").grid(
            row=2, column=0, columnspan=2, sticky="w", pady=(6, 0),
        )
        ttk.Button(status, text="重置当前任务", command=self._reset_wb_job).grid(
            row=0, column=2, rowspan=2, padx=(10, 0), sticky="ns",
        )

    def _wb_form_entry(self, parent, label, key, row, column=0, *, default="", width=34):
        ttk.Label(parent, text=label).grid(row=row, column=column, sticky="w", padx=5, pady=4)
        entry = ttk.Entry(parent, textvariable=self._var(key, default), width=width)
        entry.grid(row=row, column=column + 1, sticky="ew", padx=5, pady=4)
        return entry

    def _build_wb_product_tab(self, tab):
        tab.columnconfigure(1, weight=1)
        tab.columnconfigure(3, weight=1)

        mode_frame = ttk.LabelFrame(tab, text="WB 上品模式", padding=7)
        mode_frame.grid(row=0, column=0, columnspan=4, sticky="ew", pady=(0, 8))
        mode_frame.columnconfigure(3, weight=1)
        ttk.Label(mode_frame, text="模式").grid(row=0, column=0, sticky="w", padx=4, pady=3)
        self.wb_listing_mode_buttons = []
        for column, text, value in (
            (1, "跟卖模式", "follow"),
            (2, "本地新品模式", "local"),
        ):
            button = ttk.Radiobutton(
                mode_frame,
                text=text,
                value=value,
                variable=self._var("wb_listing_mode", "local"),
                command=self._wb_on_listing_mode_changed,
            )
            button.grid(row=0, column=column, sticky="w", padx=6, pady=3)
            self.wb_listing_mode_buttons.append(button)
        ttk.Label(mode_frame, text="WB 来源链接/商品 ID").grid(
            row=1, column=0, sticky="w", padx=4, pady=3,
        )
        self.wb_source_entry = ttk.Entry(
            mode_frame, textvariable=self._var("wb_source_url"),
        )
        self.wb_source_entry.grid(
            row=1, column=1, columnspan=3, sticky="ew", padx=4, pady=3,
        )
        self.wb_source_parse_button = ttk.Button(
            mode_frame,
            text="解析来源商品",
            command=lambda: self._run(self._wb_parse_follow_source),
        )
        self.wb_source_parse_button.grid(row=1, column=4, sticky="e", padx=4, pady=3)
        self._var("wb_source_nm_id")
        self.wb_source_status_var = tk.StringVar(value="本地新品模式：使用自己准备的成品图片")
        ttk.Label(
            mode_frame, textvariable=self.wb_source_status_var, foreground="#245a8d",
        ).grid(row=2, column=0, columnspan=5, sticky="w", padx=4, pady=(3, 0))

        self._wb_form_entry(
            tab, "卖家货号", "wb_vendor_code", 1,
            default=f"WB-{time.strftime('%Y%m%d')}-{uuid.uuid4().hex[:6].upper()}",
        )
        self._wb_form_entry(tab, "品牌（可选）", "wb_brand", 1, 2)
        self._wb_form_entry(tab, "商品标题", "wb_title", 2, width=58)
        self._wb_form_entry(tab, "1688 采购链接（可选）", "wb_supplier_1688_url", 2, 2)
        ttk.Label(tab, text="商品描述").grid(row=3, column=0, sticky="nw", padx=5, pady=4)
        self.wb_description_text = tk.Text(tab, height=7, wrap="word")
        self.wb_description_text.grid(row=3, column=1, columnspan=3, sticky="nsew", padx=5, pady=4)
        tab.rowconfigure(3, weight=1)
        size_frame = ttk.LabelFrame(tab, text="规格（WB 使用厘米和千克）", padding=7)
        size_frame.grid(row=4, column=0, columnspan=4, sticky="ew", pady=(8, 4))
        for column in (1, 3, 5, 7):
            size_frame.columnconfigure(column, weight=1)
        for column, label, key, default in (
            (0, "包装长(cm)", "wb_length_cm", "10"),
            (2, "包装宽(cm)", "wb_width_cm", "10"),
            (4, "包装高(cm)", "wb_height_cm", "10"),
            (6, "包装毛重(kg)", "wb_weight_kg", "1"),
        ):
            ttk.Label(size_frame, text=label).grid(row=0, column=column, sticky="w", padx=4, pady=3)
            ttk.Entry(size_frame, textvariable=self._var(key, default), width=13).grid(
                row=0, column=column + 1, sticky="ew", padx=4, pady=3,
            )
        for column, label, key in (
            (0, "技术尺码（卖家实际）", "wb_tech_size"),
            (2, "WB 尺码（平台展示）", "wb_size"),
            (4, "商品条码", "wb_barcode"),
        ):
            ttk.Label(size_frame, text=label).grid(row=1, column=column, sticky="w", padx=4, pady=3)
            ttk.Entry(size_frame, textvariable=self._var(key), width=18).grid(
                row=1, column=column + 1, sticky="ew", padx=4, pady=3,
            )
        ttk.Label(
            size_frame,
            text="无尺码商品两项留空；技术尺码示例 S/42，WB 尺码仅在类目要求时填写",
            foreground="#586570",
        ).grid(row=1, column=6, columnspan=2, sticky="e", padx=4)

        pricing = ttk.LabelFrame(tab, text="WB 利润率核价", padding=7)
        pricing.grid(row=5, column=0, columnspan=4, sticky="ew", pady=(6, 0))
        self._build_wb_pricing_panel(pricing)

    def _build_wb_pricing_panel(self, pricing):
        for column in (1, 3, 5, 7):
            pricing.columnconfigure(column, weight=1)
        ttk.Label(pricing, text="发货方式").grid(row=0, column=0, sticky="w", padx=4, pady=3)
        for column, text, value in (
            (1, "跨境发货", "cross_border"),
            (2, "海外仓发货", "overseas_warehouse"),
        ):
            ttk.Radiobutton(
                pricing, text=text, value=value,
                variable=self._var("wb_fulfillment_mode", "cross_border"),
                command=self._wb_on_fulfillment_mode_changed,
            ).grid(row=0, column=column, sticky="w", padx=4, pady=3)

        def pricing_entry(label, key, row, column, default=""):
            label_widget = ttk.Label(pricing, text=label)
            label_widget.grid(
                row=row, column=column, sticky="w", padx=4, pady=3,
            )
            entry = ttk.Entry(
                pricing, textvariable=self._var(key, default), width=12,
            )
            entry.grid(row=row, column=column + 1, sticky="ew", padx=4, pady=3)
            return label_widget, entry

        pricing_entry("采购成本(CNY)", "wb_purchase_cost", 1, 0)
        pricing_entry("贴标费(CNY)", "wb_label_fee", 1, 2, "0")
        pricing_entry("目标利润率(%)", "wb_target_profit_margin_percent", 1, 4, "20")
        pricing_entry("人工佣金(%)", "wb_sales_commission_percent", 1, 6)
        pricing_entry("广告费率(%)", "wb_advertising_percent", 2, 0, "15")
        pricing_entry("货损率(%)", "wb_cargo_loss_percent", 2, 2, "10")
        self.wb_cny_rub_label, self.wb_cny_rub_entry = pricing_entry(
            "1 CNY = RUB（仅海外仓）", "wb_cny_rub_rate", 2, 4, "12",
        )
        pricing_entry("折扣(%)", "wb_discount", 2, 6, "0")

        self.wb_cross_border_pricing = ttk.Frame(pricing)
        self.wb_cross_border_pricing.grid(row=3, column=0, columnspan=8, sticky="ew")
        self.wb_cross_border_fee_var = tk.StringVar(value="标准跨境运费：待计算")
        ttk.Label(
            self.wb_cross_border_pricing,
            textvariable=self.wb_cross_border_fee_var,
            foreground="#586570",
        ).grid(row=0, column=0, sticky="w", padx=4, pady=3)

        self.wb_overseas_pricing = ttk.Frame(pricing)
        for column in (1, 3, 5, 7):
            self.wb_overseas_pricing.columnconfigure(column, weight=1)

        def overseas_entry(label, key, column, default=""):
            ttk.Label(self.wb_overseas_pricing, text=label).grid(
                row=0, column=column, sticky="w", padx=4, pady=3,
            )
            ttk.Entry(
                self.wb_overseas_pricing,
                textvariable=self._var(key, default), width=12,
            ).grid(row=0, column=column + 1, sticky="ew", padx=4, pady=3)

        overseas_entry("头程运费(CNY/件)", "wb_first_mile_shipping", 0, "0")
        overseas_entry("海外仓操作费(CNY/件)", "wb_warehouse_operation_fee", 2, "0")
        overseas_entry("仓库物流系数(%)", "wb_logistics_coefficient_percent", 4, "100")
        overseas_entry("本地化指数", "wb_localization_index", 6, "1")
        self.wb_overseas_fee_var = tk.StringVar(value="海外仓物流：待计算")
        ttk.Label(
            self.wb_overseas_pricing,
            textvariable=self.wb_overseas_fee_var,
            foreground="#586570",
            wraplength=1050,
        ).grid(row=1, column=0, columnspan=8, sticky="w", padx=4, pady=3)

        self.wb_price_currency_var = tk.StringVar(value="自动标价(CNY)")
        ttk.Label(pricing, textvariable=self.wb_price_currency_var).grid(
            row=4, column=0, sticky="w", padx=4, pady=3,
        )
        self.wb_price_entry = ttk.Entry(
            pricing, textvariable=self._var("wb_price"), width=14, state="readonly",
        )
        self.wb_price_entry.grid(row=4, column=1, sticky="ew", padx=4, pady=3)
        self.wb_pricing_status_var = tk.StringVar(value="请填写完整核价参数")
        ttk.Label(
            pricing, textvariable=self.wb_pricing_status_var, foreground="#245a8d",
            wraplength=900,
        ).grid(row=4, column=2, columnspan=6, sticky="w", padx=4, pady=3)

        for key in (
            "wb_purchase_cost", "wb_label_fee", "wb_target_profit_margin_percent",
            "wb_sales_commission_percent", "wb_advertising_percent",
            "wb_cargo_loss_percent", "wb_cny_rub_rate", "wb_discount",
            "wb_first_mile_shipping", "wb_warehouse_operation_fee",
            "wb_logistics_coefficient_percent", "wb_localization_index",
            "wb_weight_kg", "wb_length_cm", "wb_width_cm", "wb_height_cm",
        ):
            self.vars[key].trace_add("write", self._wb_update_price)

    def _build_wb_category_tab(self, tab):
        tab.columnconfigure(1, weight=1)
        tab.columnconfigure(3, weight=1)
        tab.rowconfigure(3, weight=3)
        tab.rowconfigure(5, weight=2)
        ttk.Label(tab, text="父类目").grid(row=0, column=0, sticky="w", padx=5, pady=4)
        self.wb_parent_combo = ttk.Combobox(
            tab, textvariable=self._var("wb_parent_display"), state="readonly",
        )
        self.wb_parent_combo.grid(row=0, column=1, sticky="ew", padx=5, pady=4)
        self.wb_parent_combo.bind(
            "<<ComboboxSelected>>", lambda _event: self._run(self._wb_load_subjects),
        )
        ttk.Button(
            tab, text="加载父类目", command=lambda: self._run(self._wb_load_parents),
        ).grid(row=0, column=2, sticky="w", padx=5)
        ttk.Label(tab, text="子类目").grid(row=1, column=0, sticky="w", padx=5, pady=4)
        self.wb_subject_combo = ttk.Combobox(
            tab, textvariable=self._var("wb_subject_display"), state="normal",
        )
        self.wb_subject_combo.grid(row=1, column=1, sticky="ew", padx=5, pady=4)
        self.wb_subject_combo.bind("<<ComboboxSelected>>", self._wb_subject_selected)
        self.wb_subject_combo.bind("<Return>", lambda _event: self._run(self._wb_search_subjects))
        ttk.Button(
            tab, text="搜索/刷新", command=lambda: self._run(self._wb_search_subjects),
        ).grid(row=1, column=2, sticky="w", padx=5)
        ttk.Button(
            tab, text="读取类目属性", command=lambda: self._run(self._wb_load_characteristics),
        ).grid(row=1, column=3, sticky="w", padx=5)
        self._var("wb_parent_id")
        self._var("wb_subject_id")

        ttk.Label(
            tab, text="红色值表示必填；双击或选中属性后在下方编辑。多个值可填写 JSON 数组。",
            foreground="#8a5a00",
        ).grid(row=2, column=0, columnspan=4, sticky="w", padx=5, pady=(4, 6))
        self.wb_attribute_tree = ttk.Treeview(
            tab, columns=("name", "required", "type", "unit", "value"),
            show="headings", selectmode="browse", height=9,
        )
        for column, title, width in (
            ("name", "属性", 230), ("required", "必填", 60),
            ("type", "类型", 90), ("unit", "单位", 75), ("value", "当前值", 420),
        ):
            self.wb_attribute_tree.heading(column, text=title)
            self.wb_attribute_tree.column(column, width=width, stretch=column in {"name", "value"})
        self.wb_attribute_tree.tag_configure("required", foreground="#b42318")
        self.wb_attribute_tree.grid(row=3, column=0, columnspan=4, sticky="nsew", padx=5)
        attr_scroll = ttk.Scrollbar(tab, orient="vertical", command=self.wb_attribute_tree.yview)
        attr_scroll.grid(row=3, column=4, sticky="ns")
        self.wb_attribute_tree.configure(yscrollcommand=attr_scroll.set)
        self.wb_attribute_tree.bind("<<TreeviewSelect>>", self._wb_attribute_selected)
        self.wb_attribute_tree.bind("<Double-1>", self._wb_attribute_selected)

        editor = ttk.LabelFrame(tab, text="编辑选中属性", padding=7)
        editor.grid(row=4, column=0, columnspan=4, sticky="ew", padx=5, pady=(8, 0))
        editor.columnconfigure(1, weight=1)
        self.wb_selected_attribute_var = tk.StringVar(value="请先选择属性")
        ttk.Label(editor, textvariable=self.wb_selected_attribute_var).grid(
            row=0, column=0, sticky="w", padx=4,
        )
        ttk.Entry(editor, textvariable=self._var("wb_attribute_value")).grid(
            row=0, column=1, sticky="ew", padx=4,
        )
        ttk.Button(editor, text="保存属性", command=self._wb_save_characteristic).grid(row=0, column=2, padx=4)
        ttk.Button(editor, text="清空", command=self._wb_clear_characteristic).grid(row=0, column=3, padx=4)
        self.wb_directory_combo = ttk.Combobox(
            editor, textvariable=self._var("wb_directory_display"), state="readonly",
        )
        self.wb_directory_combo.grid(row=1, column=1, sticky="ew", padx=4, pady=(6, 0))
        ttk.Button(
            editor, text="加载官方选项", command=lambda: self._run(self._wb_load_directory_options),
        ).grid(row=1, column=2, padx=4, pady=(6, 0))
        ttk.Button(editor, text="采用选项", command=self._wb_use_directory_value).grid(
            row=1, column=3, padx=4, pady=(6, 0),
        )

        json_frame = ttk.LabelFrame(
            tab, text="WB 内容 JSON（包装尺寸和重量仍在商品信息中独立填写）", padding=7,
        )
        json_frame.grid(row=5, column=0, columnspan=4, sticky="nsew", padx=5, pady=(8, 0))
        json_frame.columnconfigure(0, weight=1)
        json_frame.rowconfigure(1, weight=1)
        json_toolbar = ttk.Frame(json_frame)
        json_toolbar.grid(row=0, column=0, sticky="ew", pady=(0, 6))
        ttk.Button(
            json_toolbar, text="同步当前值到 JSON", command=self._wb_sync_content_json,
        ).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(
            json_toolbar, text="应用 JSON", command=self._wb_apply_content_json,
        ).pack(side=tk.LEFT, padx=5)
        ttk.Button(
            json_toolbar, text="AI 根据图片填写",
            command=lambda: self._run(self._wb_ai_fill_content),
        ).pack(side=tk.LEFT, padx=5)
        self.wb_content_json_status_var = tk.StringVar(
            value="先读取类目属性，再由 AI 按 WB API 返回的字段填写；所有结果均可人工复核",
        )
        ttk.Label(
            json_toolbar, textvariable=self.wb_content_json_status_var, foreground="#245a8d",
        ).pack(side=tk.LEFT, padx=10)
        self.wb_content_json_text = tk.Text(json_frame, height=7, wrap="none")
        self.wb_content_json_text.grid(row=1, column=0, sticky="nsew")
        json_scroll = ttk.Scrollbar(
            json_frame, orient="vertical", command=self.wb_content_json_text.yview,
        )
        json_scroll.grid(row=1, column=1, sticky="ns")
        self.wb_content_json_text.configure(yscrollcommand=json_scroll.set)

    def _build_wb_media_tab(self, tab):
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(1, weight=1)
        toolbar = ttk.Frame(tab)
        toolbar.grid(row=0, column=0, sticky="ew", pady=(0, 6))
        self.wb_image_add_button = ttk.Button(
            toolbar, text="添加图片", command=self._wb_select_images,
        )
        self.wb_image_add_button.pack(side=tk.LEFT, padx=(0, 5))
        self.wb_image_remove_button = ttk.Button(
            toolbar, text="移除", command=self._wb_remove_image,
        )
        self.wb_image_remove_button.pack(side=tk.LEFT, padx=5)
        self.wb_image_up_button = ttk.Button(
            toolbar, text="上移", command=lambda: self._wb_move_image(-1),
        )
        self.wb_image_up_button.pack(side=tk.LEFT, padx=5)
        self.wb_image_down_button = ttk.Button(
            toolbar, text="下移", command=lambda: self._wb_move_image(1),
        )
        self.wb_image_down_button.pack(side=tk.LEFT, padx=5)
        self.wb_image_status_var = tk.StringVar(value="尚未选择图片；第 1 张作为主图")
        ttk.Label(toolbar, textvariable=self.wb_image_status_var, foreground="#245a8d").pack(
            side=tk.LEFT, padx=10,
        )
        self.wb_image_list = tk.Listbox(tab, height=13, selectmode=tk.BROWSE)
        self.wb_image_list.grid(row=1, column=0, sticky="nsew")
        media_scroll = ttk.Scrollbar(tab, orient="vertical", command=self.wb_image_list.yview)
        media_scroll.grid(row=1, column=1, sticky="ns")
        self.wb_image_list.configure(yscrollcommand=media_scroll.set)

        inventory = ttk.LabelFrame(tab, text="WB 库存", padding=8)
        inventory.grid(row=2, column=0, sticky="ew", pady=(10, 0))
        for column in (1, 3, 5):
            inventory.columnconfigure(column, weight=1)
        ttk.Label(inventory, text="库存").grid(row=0, column=0, sticky="w", padx=4, pady=3)
        ttk.Entry(
            inventory, textvariable=self._var("wb_stock", "0"), width=14,
        ).grid(row=0, column=1, sticky="ew", padx=4, pady=3)
        ttk.Button(
            inventory, text="读取仓库", command=lambda: self._run(self._wb_load_warehouses),
        ).grid(row=0, column=4, columnspan=2, sticky="e", padx=4)
        ttk.Label(inventory, text="WB 仓库").grid(row=1, column=0, sticky="w", padx=4, pady=3)
        self.wb_warehouse_combo = ttk.Combobox(
            inventory, textvariable=self._var("wb_warehouse_display"), state="readonly",
        )
        self.wb_warehouse_combo.grid(row=1, column=1, columnspan=5, sticky="ew", padx=4, pady=3)
        self.wb_warehouse_combo.bind("<<ComboboxSelected>>", self._wb_warehouse_selected)
        self._var("wb_warehouse_id")
        ttk.Label(
            inventory,
            text="商品卡片创建、图片、价格均为真实 API 操作；Sandbox 模式会跳过真实仓库库存。",
            foreground="#8a5a00",
        ).grid(row=2, column=0, columnspan=6, sticky="w", padx=4, pady=(5, 0))

    def _wb_connection_settings(self) -> dict:
        return {
            "token": self.vars["wb_token"].get().strip(),
            "sandbox": self.vars["wb_sandbox"].get().strip() == "1",
        }

    def _wb_api_from_ui(self) -> WildberriesApi:
        settings = self._ui_call(self._wb_connection_settings)
        return WildberriesApi(settings["token"], sandbox=settings["sandbox"])

    def _refresh_wb_config_state(self):
        configured = bool(self.vars.get("wb_token") and self.vars["wb_token"].get().strip())
        if configured:
            mode = "Sandbox" if self.vars["wb_sandbox"].get() == "1" else "正式环境"
            seller = self.vars.get("wb_seller_name")
            suffix = f"；{seller.get()}" if seller is not None and seller.get().strip() else ""
            self.wb_connection_var.set(f"Token 已配置；{mode}{suffix}")
            self.wb_submit_button.state(["!disabled"])
        else:
            self.wb_connection_var.set("尚未配置 WB API Token")
            self.wb_submit_button.state(["disabled"])
        self._render_wb_job()

    def _save_wb_config(self):
        try:
            settings = self._wb_connection_settings()
            WildberriesApi(settings["token"], sandbox=settings["sandbox"])
            atomic_write_json(self.WB_CONFIG_PATH, self._config())
        except Exception as error:
            messagebox.showerror("WB 配置保存失败", str(error))
            return
        self._refresh_wb_config_state()
        messagebox.showinfo("WB 配置已保存", f"WB Token 已保存在本机：{self.WB_CONFIG_PATH}")

    def _test_wb_connection(self):
        api = self._wb_api_from_ui()
        pings = api.ping()
        seller = api.seller_info()
        seller_name = str(seller.get("name") or "WB 卖家")
        seller_id = str(seller.get("sid") or seller.get("id") or "")

        def update():
            self._var("wb_seller_name").set(seller_name)
            self._var("wb_seller_id").set(seller_id)
            atomic_write_json(self.WB_CONFIG_PATH, self._config())
            self._refresh_wb_config_state()
            messagebox.showinfo(
                "WB 连接成功",
                f"卖家：{seller_name}\n已验证：{', '.join(pings)}\n"
                "上架还需要 Content、Prices and Discounts、Marketplace 的读写权限。",
            )

        self._ui_call(update)

    @staticmethod
    def _wb_parent_identity(item: dict) -> tuple[int, str]:
        return int(item.get("id") or item.get("parentID") or 0), str(
            item.get("name") or item.get("parentName") or "未命名父类目"
        )

    @staticmethod
    def _wb_subject_identity(item: dict) -> tuple[int, str]:
        return int(item.get("subjectID") or item.get("id") or 0), str(
            item.get("subjectName") or item.get("name") or "未命名子类目"
        )

    def _wb_load_parents(self):
        parents = self._wb_api_from_ui().parent_categories()

        def update():
            self.wb_parents = parents
            self.wb_parent_by_display = {}
            for item in parents:
                parent_id, name = self._wb_parent_identity(item)
                if parent_id:
                    self.wb_parent_by_display[f"{name} [{parent_id}]"] = item
            values = list(self.wb_parent_by_display)
            self.wb_parent_combo.configure(values=values)
            self.wb_status_var.set(f"已读取 {len(values)} 个 WB 父类目")

        self._ui_call(update)

    def _wb_selected_parent(self) -> tuple[int, str]:
        display = self.vars["wb_parent_display"].get().strip()
        item = self.wb_parent_by_display.get(display, {})
        if item:
            return self._wb_parent_identity(item)
        try:
            return int(self.vars["wb_parent_id"].get()), display
        except ValueError:
            return 0, display

    def _wb_load_subjects(self):
        parent_id, _display = self._ui_call(self._wb_selected_parent)
        if not parent_id:
            raise ValueError("请先选择 WB 父类目")
        subjects = self._wb_api_from_ui().subjects(parent_id=parent_id)
        self._ui_call(lambda: self._wb_apply_subjects(subjects, parent_id))

    def _wb_search_subjects(self):
        parent_id, _display = self._ui_call(self._wb_selected_parent)
        search_text = self._ui_call(lambda: self.vars["wb_subject_display"].get().strip())
        if search_text in self.wb_subject_by_display:
            search_text = ""
        subjects = self._wb_api_from_ui().subjects(
            parent_id=parent_id or None, name=search_text,
        )
        self._ui_call(lambda: self._wb_apply_subjects(subjects, parent_id))

    def _wb_apply_subjects(self, subjects: list[dict], parent_id=0):
        self.wb_subjects = subjects
        self.wb_subject_by_display = {}
        for item in subjects:
            subject_id, name = self._wb_subject_identity(item)
            if subject_id:
                self.wb_subject_by_display[f"{name} [{subject_id}]"] = item
        self.wb_subject_combo.configure(values=list(self.wb_subject_by_display))
        if parent_id:
            self.vars["wb_parent_id"].set(str(parent_id))
        self.wb_status_var.set(f"已读取 {len(subjects)} 个 WB 子类目")

    def _wb_subject_selected(self, _event=None):
        display = self.vars["wb_subject_display"].get().strip()
        item = self.wb_subject_by_display.get(display)
        if not item:
            return
        subject_id, _name = self._wb_subject_identity(item)
        try:
            current_subject_id = int(self.vars["wb_subject_id"].get().strip())
        except ValueError:
            current_subject_id = 0
        if current_subject_id == subject_id:
            return
        self.vars["wb_subject_id"].set(str(subject_id))
        self.wb_characteristics = []
        self.wb_characteristic_values = {}
        self._wb_refresh_characteristics_tree()
        self._wb_sync_content_json(show_error=False)

    def _wb_selected_subject_id(self) -> int:
        self._wb_subject_selected()
        try:
            subject_id = int(self.vars["wb_subject_id"].get().strip())
        except ValueError as error:
            raise ValueError("请先选择有效的 WB 子类目") from error
        if subject_id <= 0:
            raise ValueError("请先选择有效的 WB 子类目")
        return subject_id

    def _wb_load_characteristics(self):
        subject_id = self._ui_call(self._wb_selected_subject_id)
        characteristics = self._wb_api_from_ui().subject_characteristics(subject_id)

        def update():
            self.wb_characteristics = characteristics
            self.wb_characteristic_values = {
                str(key): value for key, value in self.wb_characteristic_values.items()
                if any(str(item.get("charcID")) == str(key) for item in characteristics)
            }
            self._wb_refresh_characteristics_tree()
            self._wb_sync_content_json(show_error=False)
            missing = sum(1 for item in characteristics if item.get("required"))
            self.wb_status_var.set(f"类目属性 {len(characteristics)} 项；必填 {missing} 项")

        self._ui_call(update)

    def _wb_refresh_characteristics_tree(self):
        tree = getattr(self, "wb_attribute_tree", None)
        if tree is None:
            return
        tree.delete(*tree.get_children())
        for item in self.wb_characteristics:
            charc_id = str(item.get("charcID") or "")
            if not charc_id:
                continue
            value = self.wb_characteristic_values.get(charc_id, "")
            rendered = json.dumps(value, ensure_ascii=False) if isinstance(value, (list, dict)) else str(value)
            required = bool(item.get("required"))
            tree.insert(
                "", tk.END, iid=charc_id,
                values=(
                    item.get("name") or charc_id,
                    "是" if required else "否",
                    item.get("charcType") if item.get("charcType") is not None else "",
                    item.get("unitName") or "",
                    rendered,
                ),
                tags=("required",) if required and not rendered.strip() else (),
            )

    def _wb_selected_characteristic(self) -> dict:
        selection = self.wb_attribute_tree.selection()
        if not selection:
            raise ValueError("请先在属性表中选择一项")
        charc_id = str(selection[0])
        return next(
            (item for item in self.wb_characteristics if str(item.get("charcID")) == charc_id),
            {},
        )

    def _wb_attribute_selected(self, _event=None):
        try:
            item = self._wb_selected_characteristic()
        except ValueError:
            return
        charc_id = str(item.get("charcID"))
        self.wb_selected_attribute_var.set(f"{item.get('name') or charc_id} [{charc_id}]")
        value = self.wb_characteristic_values.get(charc_id, "")
        self.vars["wb_attribute_value"].set(
            json.dumps(value, ensure_ascii=False) if isinstance(value, (list, dict)) else str(value)
        )
        self.vars["wb_directory_display"].set("")
        self.wb_directory_combo.configure(values=[])

    def _wb_save_characteristic(self):
        try:
            item = self._wb_selected_characteristic()
        except Exception as error:
            messagebox.showwarning("未选择属性", str(error))
            return
        charc_id = str(item.get("charcID"))
        raw_value = self.vars["wb_attribute_value"].get().strip()
        if raw_value:
            self.wb_characteristic_values[charc_id] = raw_value
        else:
            self.wb_characteristic_values.pop(charc_id, None)
        self._wb_refresh_characteristics_tree()
        self._wb_sync_content_json(show_error=False)

    def _wb_clear_characteristic(self):
        try:
            item = self._wb_selected_characteristic()
        except ValueError:
            return
        self.wb_characteristic_values.pop(str(item.get("charcID")), None)
        self.vars["wb_attribute_value"].set("")
        self._wb_refresh_characteristics_tree()
        self._wb_sync_content_json(show_error=False)

    @staticmethod
    def _wb_directory_kind(name: str) -> str:
        normalized = str(name or "").casefold()
        mappings = (
            ("colors", ("color", "цвет", "颜色")),
            ("kinds", ("gender", "kind", "пол", "性别")),
            ("countries", ("country", "страна", "国家")),
            ("seasons", ("season", "сезон", "季节")),
            ("vat", ("vat", "ндс", "增值税", "税率")),
            ("tnved", ("tnved", "тнвэд", "海关", "商品编码")),
        )
        for kind, needles in mappings:
            if any(needle in normalized for needle in needles):
                return kind
        raise ValueError("该属性没有可识别的 WB 官方字典，请手工填写属性值")

    def _wb_load_directory_options(self):
        item = self._ui_call(self._wb_selected_characteristic)
        kind = self._wb_directory_kind(str(item.get("name") or ""))
        subject_id = self._ui_call(self._wb_selected_subject_id)
        values = self._wb_api_from_ui().directory_values(kind, subject_id=subject_id)

        def update():
            self.wb_directory_by_display = {}
            for raw in values:
                if isinstance(raw, dict):
                    value = next((
                        raw.get(key) for key in ("name", "value", "title", "tnved", "id")
                        if raw.get(key) not in (None, "")
                    ), raw)
                    display = str(value)
                    if display in self.wb_directory_by_display:
                        display += " | " + json.dumps(raw, ensure_ascii=False)
                else:
                    value = raw
                    display = str(raw)
                self.wb_directory_by_display[display] = value
            choices = list(self.wb_directory_by_display)
            self.wb_directory_combo.configure(values=choices)
            if choices:
                self.wb_directory_combo.current(0)
            self.wb_status_var.set(f"已读取 {len(choices)} 个官方属性选项")

        self._ui_call(update)

    def _wb_use_directory_value(self):
        display = self.vars["wb_directory_display"].get().strip()
        if display not in self.wb_directory_by_display:
            messagebox.showwarning("未选择选项", "请先加载并选择一个 WB 官方选项")
            return
        self.vars["wb_attribute_value"].set(
            json.dumps([self.wb_directory_by_display[display]], ensure_ascii=False)
        )
        self._wb_save_characteristic()

    def _wb_content_form_inputs(self) -> dict:
        return {
            "subject_id": self.vars["wb_subject_id"].get().strip(),
            "vendor_code": self.vars["wb_vendor_code"].get().strip(),
            "title": self.vars["wb_title"].get().strip(),
            "description": self.wb_description_text.get("1.0", tk.END).strip(),
            "brand": self.vars["wb_brand"].get().strip(),
            "barcode": self.vars["wb_barcode"].get().strip(),
            "tech_size": self.vars["wb_tech_size"].get().strip(),
            "wb_size": self.vars["wb_size"].get().strip(),
            "characteristic_definitions": list(self.wb_characteristics),
            "characteristic_values": dict(self.wb_characteristic_values),
        }

    def _wb_write_content_json(self, document: dict):
        if not hasattr(self, "wb_content_json_text"):
            return
        self.wb_content_json_text.delete("1.0", tk.END)
        self.wb_content_json_text.insert(
            "1.0", json.dumps(document, ensure_ascii=False, indent=2),
        )

    def _wb_sync_content_json(self, *, show_error=True):
        try:
            document = build_card_content_document(self._wb_content_form_inputs())
        except Exception as error:
            if show_error:
                messagebox.showwarning("暂时无法生成 JSON", str(error))
            return None
        self._wb_write_content_json(document)
        return document

    def _wb_apply_content_values(self, values: dict, document: dict | None = None):
        for key in ("vendor_code", "title", "brand", "barcode", "tech_size", "wb_size"):
            self.vars["wb_" + key].set(str(values.get(key) or ""))
        self.wb_description_text.delete("1.0", tk.END)
        self.wb_description_text.insert("1.0", str(values.get("description") or ""))
        self.wb_characteristic_values = dict(values.get("characteristic_values") or {})
        self._wb_refresh_characteristics_tree()
        if document is None:
            document = build_card_content_document(self._wb_content_form_inputs())
        self._wb_write_content_json(document)
        missing = [
            str(item.get("name") or item.get("charcID"))
            for item in self.wb_characteristics
            if item.get("required") and not self.wb_characteristic_values.get(str(item.get("charcID")))
        ]
        if missing:
            self.wb_content_json_status_var.set(
                f"已应用内容；仍有 {len(missing)} 项必填属性需要人工核对",
            )
        else:
            self.wb_content_json_status_var.set("内容已应用，当前必填属性均已有值")

    def _wb_apply_content_json(self):
        try:
            document = json.loads(self.wb_content_json_text.get("1.0", tk.END))
            subject_id = self._wb_selected_subject_id()
            values = parse_card_content_document(
                document, self.wb_characteristics, expected_subject_id=subject_id,
            )
            self._wb_apply_content_values(values, document)
        except Exception as error:
            messagebox.showerror("WB 内容 JSON 无法应用", str(error))

    def _wb_ai_fill_content(self):
        snapshot = self._ui_call(lambda: {
            "subject_id": self._wb_selected_subject_id(),
            "subject_name": self.vars["wb_subject_display"].get().strip(),
            "definitions": list(self.wb_characteristics),
            "image_paths": list(self.wb_image_paths),
            "document": build_card_content_document(self._wb_content_form_inputs()),
            "source_context": {
                "source_reference": dict(self.wb_source_reference),
                "supplier_1688_url": self.vars["wb_supplier_1688_url"].get().strip(),
            },
            "config": self._analysis_config(),
        })
        service = WbCardAiService(**snapshot["config"])
        self._log(
            f"开始使用 {min(len(snapshot['image_paths']), service.MAX_IMAGES)} 张图片填写 WB 内容和类目属性",
        )
        document = service.generate(
            subject_id=snapshot["subject_id"],
            subject_name=snapshot["subject_name"],
            definitions=snapshot["definitions"],
            image_paths=snapshot["image_paths"],
            current_document=snapshot["document"],
            source_context=snapshot["source_context"],
        )
        values = parse_card_content_document(
            document, snapshot["definitions"], expected_subject_id=snapshot["subject_id"],
        )

        def update():
            self._wb_apply_content_values(values, document)
            self.wb_status_var.set("WB 图片 AI 已填写内容；请重点核对红色必填属性和尺码")

        self._ui_call(update)
        self._log("WB 图片 AI 填写完成，结果已写入可编辑 JSON")

    def _wb_on_listing_mode_changed(self):
        if not hasattr(self, "wb_source_entry"):
            return
        mode = self.vars["wb_listing_mode"].get().strip() or "local"
        if mode not in {"follow", "local"}:
            mode = "local"
            self.vars["wb_listing_mode"].set(mode)
        is_follow = mode == "follow"
        self.wb_source_entry.configure(state="normal" if is_follow else "disabled")
        self.wb_source_parse_button.state(["!disabled"] if is_follow else ["disabled"])
        image_buttons = (
            getattr(self, "wb_image_add_button", None),
            getattr(self, "wb_image_remove_button", None),
            getattr(self, "wb_image_up_button", None),
            getattr(self, "wb_image_down_button", None),
        )
        for button in image_buttons:
            if button is not None:
                button.state(["disabled"] if is_follow else ["!disabled"])

        has_job = bool(isinstance(self.wb_job, dict) and self.wb_job.get("task_id"))
        for button in getattr(self, "wb_listing_mode_buttons", []):
            button.state(["disabled"] if has_job else ["!disabled"])
        if is_follow:
            source_id = self.vars.get("wb_source_nm_id")
            if source_id is not None and source_id.get().strip():
                self.wb_source_status_var.set(
                    f"跟卖模式：已解析 WB 商品 {source_id.get().strip()}，来源图片将用于新商品卡片"
                )
            else:
                self.wb_source_status_var.set("跟卖模式：填写 WB 链接或商品 ID 后解析来源商品")
        else:
            self.wb_source_status_var.set("本地新品模式：不需要跟卖链接，只使用自己准备的成品图片")

    @staticmethod
    def _wb_calculate_pricing(inputs: dict) -> WbPricingBreakdown:
        return calculate_wb_price(
            fulfillment_mode=inputs.get("fulfillment_mode"),
            purchase_cost_cny=inputs.get("purchase_cost"),
            label_fee_cny=inputs.get("label_fee"),
            target_profit_margin_percent=inputs.get("target_profit_margin_percent"),
            sales_commission_percent=inputs.get("sales_commission_percent"),
            advertising_percent=inputs.get("advertising_percent"),
            cargo_loss_percent=inputs.get("cargo_loss_percent"),
            cny_rub_rate=inputs.get("cny_rub_rate"),
            weight_kg=inputs.get("weight_kg"),
            length_cm=inputs.get("length_cm"),
            width_cm=inputs.get("width_cm"),
            height_cm=inputs.get("height_cm"),
            discount_percent=inputs.get("discount", 0),
            first_mile_shipping_cny=inputs.get("first_mile_shipping", 0),
            warehouse_operation_fee_cny=inputs.get("warehouse_operation_fee", 0),
            logistics_coefficient_percent=inputs.get("logistics_coefficient_percent", 100),
            localization_index=inputs.get("localization_index", 1),
        )

    def _wb_pricing_form_values(self) -> dict[str, str]:
        keys = (
            "fulfillment_mode", "purchase_cost", "label_fee",
            "target_profit_margin_percent", "sales_commission_percent",
            "advertising_percent", "cargo_loss_percent", "cny_rub_rate",
            "weight_kg", "length_cm", "width_cm", "height_cm", "discount",
            "first_mile_shipping", "warehouse_operation_fee",
            "logistics_coefficient_percent", "localization_index",
        )
        return {
            key: self.vars["wb_" + key].get().strip()
            for key in keys
        }

    @staticmethod
    def _wb_money(value: Decimal) -> str:
        return format(value.quantize(Decimal("0.01")), "f")

    def _wb_update_price(self, *_args):
        if self._wb_pricing_trace_suspended or not hasattr(self, "wb_pricing_status_var"):
            return None
        try:
            result = self._wb_calculate_pricing(self._wb_pricing_form_values())
        except Exception as error:
            self.wb_pricing_breakdown = None
            self._wb_pricing_trace_suspended = True
            try:
                self.vars["wb_price"].set("")
            finally:
                self._wb_pricing_trace_suspended = False
            self.wb_pricing_status_var.set(str(error))
            if hasattr(self, "wb_cross_border_fee_var"):
                self.wb_cross_border_fee_var.set("标准跨境运费：待计算")
            if hasattr(self, "wb_overseas_fee_var"):
                self.wb_overseas_fee_var.set("海外仓物流：待计算")
            return None

        self.wb_pricing_breakdown = result
        self._wb_pricing_trace_suspended = True
        try:
            self.vars["wb_price"].set(format(result.list_price, "f"))
        finally:
            self._wb_pricing_trace_suspended = False
        if result.fulfillment_mode == "cross_border":
            self.wb_cross_border_fee_var.set(
                f"标准跨境运费：{self._wb_money(result.cross_border_shipping_cny)} CNY"
                f"（计费重 {format(result.billable_weight_kg, 'f')}kg）"
            )
        else:
            head_cny = result.first_mile_shipping_cny + result.warehouse_operation_fee_cny
            self.wb_overseas_fee_var.set(
                f"头程合计 {self._wb_money(head_cny)} CNY | "
                f"WB 尾程 {self._wb_money(result.tail_delivery_rub)} RUB | "
                f"包装体积 {self._wb_money(result.volume_liters)} L"
            )
        self.wb_pricing_status_var.set(
            f"折后 {self._wb_money(result.discounted_price)} {result.currency_code} | "
            f"总物流 {self._wb_money(result.logistics_total)} {result.currency_code} | "
            f"利润 {self._wb_money(result.profit)} {result.currency_code} | "
            f"利润率 {self._wb_money(result.actual_profit_margin_percent)}%"
        )
        return result

    def _wb_on_fulfillment_mode_changed(self, *_args):
        if not hasattr(self, "wb_pricing_status_var"):
            return
        mode = self.vars["wb_fulfillment_mode"].get().strip() or "cross_border"
        if mode not in {"cross_border", "overseas_warehouse"}:
            mode = "cross_border"
            self.vars["wb_fulfillment_mode"].set(mode)
        currency_code = "RUB" if mode == "overseas_warehouse" else "CNY"
        if hasattr(self, "wb_price_currency_var"):
            self.wb_price_currency_var.set(f"自动标价({currency_code})")
        if hasattr(self, "wb_cny_rub_entry"):
            if mode == "overseas_warehouse":
                self.wb_cny_rub_entry.state(["!disabled"])
            else:
                self.wb_cny_rub_entry.state(["disabled"])
        if hasattr(self, "wb_cross_border_pricing"):
            if mode == "overseas_warehouse":
                self.wb_cross_border_pricing.grid_remove()
                self.wb_overseas_pricing.grid(row=3, column=0, columnspan=8, sticky="ew")
            else:
                self.wb_overseas_pricing.grid_remove()
                self.wb_cross_border_pricing.grid(row=3, column=0, columnspan=8, sticky="ew")
        self._wb_update_price()

    def _wb_parse_follow_source(self):
        source = self._ui_call(lambda: self.vars["wb_source_url"].get().strip())
        mode = self._ui_call(lambda: self.vars["wb_listing_mode"].get().strip())
        if mode != "follow":
            raise ValueError("只有 WB 跟卖模式需要解析来源商品")
        canonical_url, nm_id = normalize_wb_source(source)
        self._log(f"开始解析 WB 来源商品：{canonical_url}")
        product = scrape_wb_source(
            canonical_url,
            profile_dir=self.OUTPUT_DIR.parent / "browser_profile_wb",
            log_func=self._log,
        )
        image_dir = (
            self.OUTPUT_DIR / "wb-source" / nm_id /
            f"{time.strftime('%Y%m%d-%H%M%S')}-{uuid.uuid4().hex[:6]}"
        )
        image_paths = download_wb_source_images(
            product.images, image_dir, log_func=self._log,
        )
        reference = product.to_dict()
        reference["downloaded_image_paths"] = list(image_paths)

        def update():
            if self.vars["wb_listing_mode"].get().strip() != "follow":
                return
            self.wb_source_reference = reference
            self.vars["wb_source_url"].set(product.url)
            self.vars["wb_source_nm_id"].set(product.nm_id)
            self.vars["wb_title"].set(product.title)
            if product.brand:
                self.vars["wb_brand"].set(product.brand)
            self.wb_description_text.delete("1.0", tk.END)
            self.wb_description_text.insert("1.0", product.description)
            self._wb_set_image_paths(image_paths)
            self.wb_source_status_var.set(
                f"已解析 WB 商品 {product.nm_id}：{len(image_paths)} 张来源图片；请核对文案、类目和属性"
            )
            self.wb_status_var.set("WB 跟卖来源已准备，等待补充类目、属性、价格和库存")

        self._ui_call(update)
        self._log(f"WB 来源商品解析完成：nmID={nm_id}，图片 {len(image_paths)} 张")

    def _wb_set_image_paths(self, paths):
        unique = []
        seen = set()
        for raw in paths:
            path = str(Path(raw))
            key = path.casefold()
            if key in seen:
                continue
            seen.add(key)
            unique.append(path)
            if len(unique) >= 30:
                break
        self.wb_image_paths = unique
        self.wb_image_list.delete(0, tk.END)
        for index, path in enumerate(unique, start=1):
            self.wb_image_list.insert(tk.END, f"{index:02d}. {path}")
        mode = self.vars.get("wb_listing_mode")
        prefix = "来源" if mode is not None and mode.get().strip() == "follow" else "本地"
        self.wb_image_status_var.set(
            f"已准备 {len(unique)} 张{prefix}图片；第 1 张为主图"
            if unique else f"尚未准备{prefix}图片；第 1 张作为主图"
        )

    def _wb_select_images(self):
        if self.vars["wb_listing_mode"].get().strip() != "local":
            messagebox.showwarning("当前为跟卖模式", "跟卖模式请通过“解析来源商品”准备图片")
            return
        paths = filedialog.askopenfilenames(
            title="选择 WB 商品图片（第 1 张为主图）",
            filetypes=[("图片", "*.jpg *.jpeg *.png *.webp *.bmp *.gif")],
        )
        if paths:
            self._wb_set_image_paths([*self.wb_image_paths, *paths])

    def _wb_remove_image(self):
        selection = self.wb_image_list.curselection()
        if selection:
            paths = list(self.wb_image_paths)
            paths.pop(selection[0])
            self._wb_set_image_paths(paths)

    def _wb_move_image(self, offset: int):
        selection = self.wb_image_list.curselection()
        if not selection:
            return
        old_index = selection[0]
        new_index = max(0, min(len(self.wb_image_paths) - 1, old_index + offset))
        if new_index == old_index:
            return
        paths = list(self.wb_image_paths)
        paths.insert(new_index, paths.pop(old_index))
        self._wb_set_image_paths(paths)
        self.wb_image_list.selection_set(new_index)

    def _wb_load_warehouses(self):
        api = self._wb_api_from_ui()
        warehouses = api.warehouses()

        def update():
            self.wb_warehouse_by_display = {}
            for item in warehouses:
                warehouse_id = int(item.get("id") or 0)
                if warehouse_id:
                    display = f"{item.get('name') or '未命名仓库'} [{warehouse_id}]"
                    self.wb_warehouse_by_display[display] = item
            values = list(self.wb_warehouse_by_display)
            self.wb_warehouse_combo.configure(values=values)
            if api.sandbox:
                self.vars["wb_warehouse_id"].set("")
                self.vars["wb_warehouse_display"].set("Sandbox 不写真实库存")
                self.wb_status_var.set("Sandbox 模式已跳过真实仓库读取")
            else:
                self.wb_status_var.set(f"已读取 {len(values)} 个 WB 卖家仓库")

        self._ui_call(update)

    def _wb_warehouse_selected(self, _event=None):
        item = self.wb_warehouse_by_display.get(self.vars["wb_warehouse_display"].get().strip())
        if item:
            self.vars["wb_warehouse_id"].set(str(item.get("id") or ""))

    def _wb_collect_form_snapshot(self) -> dict:
        self._wb_subject_selected()
        self._wb_warehouse_selected()
        keys = (
            "wb_listing_mode", "wb_source_url", "wb_source_nm_id",
            "wb_vendor_code", "wb_supplier_1688_url", "wb_title", "wb_brand",
            "wb_subject_id", "wb_subject_display", "wb_length_cm", "wb_width_cm",
            "wb_height_cm", "wb_weight_kg", "wb_tech_size", "wb_size", "wb_barcode",
            "wb_price", "wb_discount", "wb_warehouse_id", "wb_warehouse_display", "wb_stock",
            "wb_seller_name", "wb_seller_id", "wb_fulfillment_mode",
            "wb_sales_commission_percent",
            "wb_purchase_cost", "wb_label_fee", "wb_target_profit_margin_percent",
            "wb_advertising_percent", "wb_cargo_loss_percent", "wb_cny_rub_rate",
            "wb_first_mile_shipping", "wb_warehouse_operation_fee",
            "wb_logistics_coefficient_percent", "wb_localization_index",
        )
        values = {key.removeprefix("wb_"): self.vars[key].get().strip() for key in keys}
        values["description"] = self.wb_description_text.get("1.0", tk.END).strip()
        values["characteristic_definitions"] = list(self.wb_characteristics)
        values["characteristic_values"] = dict(self.wb_characteristic_values)
        values["source_reference"] = dict(self.wb_source_reference)
        if values["listing_mode"] != "follow":
            values["source_url"] = ""
            values["source_nm_id"] = ""
            values["source_reference"] = {}
        return values

    @staticmethod
    def _wb_validate_listing_inputs(inputs: dict, *, sandbox: bool):
        listing_mode = str(inputs.get("listing_mode") or "local").strip()
        if listing_mode not in {"follow", "local"}:
            raise ValueError("WB 上品模式无效，请重新选择跟卖模式或本地新品模式")
        if listing_mode == "follow":
            source_url, source_nm_id = normalize_wb_source(str(inputs.get("source_url") or ""))
            supplied_nm_id = str(inputs.get("source_nm_id") or "").strip()
            if supplied_nm_id and supplied_nm_id != source_nm_id:
                raise ValueError("WB 来源链接与已解析商品 ID 不一致，请重新解析来源商品")
            reference = inputs.get("source_reference")
            reference_nm_id = str(reference.get("nm_id") or "").strip() if isinstance(reference, dict) else ""
            if reference_nm_id != source_nm_id:
                raise ValueError("请先点击“解析来源商品”，再开始 WB 跟卖上架")
            inputs["source_url"] = source_url
            inputs["source_nm_id"] = source_nm_id
        fulfillment_mode = str(inputs.get("fulfillment_mode") or "cross_border").strip()
        if fulfillment_mode not in {"cross_border", "overseas_warehouse"}:
            raise ValueError("WB 核价方式无效，请重新选择跨境发货或海外仓发货")
        pricing = WbUiMixin._wb_calculate_pricing(inputs)
        inputs["price"] = format(pricing.list_price, "f")
        inputs["currency_code"] = pricing.currency_code
        inputs["pricing_breakdown"] = pricing.json_values()
        preview = dict(inputs)
        preview["barcode"] = preview.get("barcode") or "PREVIEW-BARCODE"
        build_card_payload(preview)
        try:
            price = Decimal(str(inputs.get("price") or "").strip())
            discount = int(str(inputs.get("discount") or "0").strip())
            stock = int(str(inputs.get("stock") or "0").strip())
        except (InvalidOperation, ValueError) as error:
            raise ValueError("WB 售价、折扣和库存必须是有效整数") from error
        if not price.is_finite() or price <= 0 or price != price.to_integral_value():
            raise ValueError(f"WB 售价必须是大于 0 的整数 {pricing.currency_code}")
        if discount < 0 or discount >= 100:
            raise ValueError("WB 折扣必须在 0 到 99 之间")
        if stock < 0:
            raise ValueError("WB 库存不能小于 0")
        if not sandbox:
            try:
                warehouse_id = int(str(inputs.get("warehouse_id") or "").strip())
            except ValueError as error:
                raise ValueError("正式环境必须选择 WB 卖家仓库") from error
            if warehouse_id <= 0:
                raise ValueError("正式环境必须选择 WB 卖家仓库")

    def _wb_copy_stable_images(self, image_paths: list[str], vendor_code: str) -> list[str]:
        safe_vendor = re.sub(r"[^A-Za-z0-9._-]+", "-", vendor_code).strip("-.") or "wb-product"
        target = self.OUTPUT_DIR / "wb-products" / safe_vendor / time.strftime("%Y%m%d-%H%M%S")
        target.mkdir(parents=True, exist_ok=True)
        copied = []
        for index, raw_path in enumerate(image_paths, start=1):
            source = Path(raw_path)
            suffix = source.suffix.casefold() or ".jpg"
            destination = target / f"{index:02d}{suffix}"
            shutil.copy2(source, destination)
            copied.append(str(destination))
        return copied

    def _confirm_wb_submit(self):
        if not self.vars["wb_token"].get().strip():
            messagebox.showwarning("未配置 WB", "请先填写并保存 WB API Token")
            return
        listing_label = (
            "跟卖模式" if self.vars["wb_listing_mode"].get().strip() == "follow" else "本地新品模式"
        )
        fulfillment_label = (
            "海外仓发货"
            if self.vars["wb_fulfillment_mode"].get().strip() == "overseas_warehouse"
            else "跨境发货"
        )
        if not messagebox.askyesno(
            "确认 WB 上架",
            f"当前为 {listing_label} / {fulfillment_label}，人工佣金 "
            f"{self.vars['wb_sales_commission_percent'].get().strip() or '未填写'}%，"
            f"目标利润率 {self.vars['wb_target_profit_margin_percent'].get().strip() or '未填写'}%，"
            f"自动标价 {self.vars['wb_price'].get().strip() or '未计算'} RUB。\n"
            "将通过 WB API 创建或继续商品卡片，并上传图片、设置价格和库存。\n"
            "程序会按卖家货号防止重复创建。确认继续吗？",
        ):
            return
        self._run(self._wb_run_pipeline)

    def _wb_run_pipeline(self):
        settings = self._ui_call(self._wb_connection_settings)
        inputs = self._ui_call(self._wb_collect_form_snapshot)
        selected_images = self._ui_call(lambda: list(self.wb_image_paths))
        self._wb_validate_listing_inputs(inputs, sandbox=settings["sandbox"])
        validate_image_paths(selected_images)
        vendor_code = inputs["vendor_code"]
        state = self.wb_job if isinstance(self.wb_job, dict) else {}
        if state:
            existing_vendor = str((state.get("inputs") or {}).get("vendor_code") or "")
            if existing_vendor != vendor_code:
                raise ValueError(
                    f"当前未结束任务的货号是 {existing_vendor}；请继续该任务或先点击“重置当前任务”"
                )
            if state.get("stage") == "completed":
                raise ValueError("该 WB 任务已经完成；新建商品前请点击“重置当前任务”")
            stage = state.get("stage") or "new"
            if stage == "new":
                state["version"] = 4
                state["inputs"] = inputs
                state["image_paths"] = self._wb_copy_stable_images(selected_images, vendor_code)
            elif stage in {"card_submitted", "card_ready", "media_uploaded"}:
                state["inputs"].update({
                    key: inputs[key] for key in (
                        "price", "discount", "fulfillment_mode", "purchase_cost",
                        "label_fee", "target_profit_margin_percent",
                        "sales_commission_percent", "advertising_percent",
                        "cargo_loss_percent", "cny_rub_rate", "weight_kg",
                        "length_cm", "width_cm", "height_cm",
                        "first_mile_shipping", "warehouse_operation_fee",
                        "logistics_coefficient_percent", "localization_index",
                        "pricing_breakdown",
                    )
                })
                state["version"] = 4
            elif stage == "price_ready":
                state["inputs"].update({
                    key: inputs[key] for key in ("warehouse_id", "warehouse_display", "stock")
                })
        else:
            state = {
                "version": 4,
                "task_id": uuid.uuid4().hex,
                "stage": "new",
                "status": "queued",
                "inputs": inputs,
                "image_paths": self._wb_copy_stable_images(selected_images, vendor_code),
                "created_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            }
        self.wb_job = state
        self._save_wb_job(state)
        api = WildberriesApi(settings["token"], sandbox=settings["sandbox"])
        workflow = WbListingWorkflow(
            api, save_state=self._save_wb_job, log=self._log,
        )
        try:
            result = workflow.advance(state)
        except Exception as error:
            state["status"] = "failed"
            state["error"] = str(error) or type(error).__name__
            self._save_wb_job(state)
            raise
        if result.get("stage") == "completed" and not result.get("ledger_completed"):
            ledger = upsert_product_ledger(
                self.PRODUCT_LEDGER_PATH, self._wb_product_ledger_record(result),
            )
            result["ledger_completed"] = True
            result["ledger_path"] = ledger["path"]
            self._save_wb_job(result)
            self._log(f"WB 产品台账已更新：{ledger['path']}（第 {ledger['row']} 行）")
            self._ui_call(lambda: messagebox.showinfo(
                "WB 上架完成",
                f"nmID：{result.get('nm_id')}\nchrtID：{result.get('chrt_id')}\n"
                f"产品台账：{ledger['path']}",
            ))
        return result

    def _wb_product_ledger_record(self, state: dict) -> dict:
        inputs = state.get("inputs") or {}
        pricing = (
            inputs.get("pricing_breakdown")
            if isinstance(inputs.get("pricing_breakdown"), dict) else {}
        )

        def scaled(key, multiplier):
            value = str(inputs.get(key) or "").strip()
            if not value:
                return ""
            return str(Decimal(value) * Decimal(str(multiplier)))

        seller_id = str(inputs.get("seller_id") or "")
        listing_mode = str(inputs.get("listing_mode") or "local")
        fulfillment_mode = str(inputs.get("fulfillment_mode") or "cross_border")
        return {
            "platform": "Wildberries",
            "shop_name": inputs.get("seller_name") or "WB 卖家",
            "shop_config_id": "wb:" + (seller_id or "default"),
            "listing_mode": "WB 跟卖" if listing_mode == "follow" else "WB 本地新品",
            "offer_id": inputs.get("vendor_code"),
            "supplier_1688_url": inputs.get("supplier_1688_url"),
            "wb_source_url": inputs.get("source_url"),
            "wb_source_nm_id": inputs.get("source_nm_id"),
            "wb_nm_id": state.get("nm_id"),
            "wb_chrt_id": state.get("chrt_id"),
            "wb_subject_id": inputs.get("subject_id"),
            "title": inputs.get("title"),
            "price": inputs.get("price"),
            "currency_code": pricing.get("currency_code") or (
                "RUB" if fulfillment_mode == "overseas_warehouse" else "CNY"
            ),
            "pricing_mode": (
                "WB 海外仓发货（利润率）"
                if fulfillment_mode == "overseas_warehouse"
                else "WB 跨境发货（利润率）"
            ),
            "purchase_cost": inputs.get("purchase_cost"),
            "label_fee": inputs.get("label_fee"),
            "target_profit_margin_percent": inputs.get("target_profit_margin_percent"),
            "actual_profit_margin_percent": pricing.get("actual_profit_margin_percent"),
            "discount_percent": inputs.get("discount"),
            "advertising_percent": inputs.get("advertising_percent"),
            "cargo_loss_percent": inputs.get("cargo_loss_percent"),
            "raw_sales_commission_percent": inputs.get("sales_commission_percent"),
            "sales_commission_percent": inputs.get("sales_commission_percent"),
            "shipping": pricing.get("logistics_total") or pricing.get("logistics_total_rub"),
            "cny_rub_rate": inputs.get("cny_rub_rate"),
            "cross_border_shipping_cny": pricing.get("cross_border_shipping_cny"),
            "first_mile_shipping_cny": pricing.get("first_mile_shipping_cny"),
            "warehouse_operation_fee_cny": pricing.get("warehouse_operation_fee_cny"),
            "tail_delivery_rub": pricing.get("tail_delivery_rub"),
            "volume_liters": pricing.get("volume_liters"),
            "sales_commission": pricing.get("sales_commission") or pricing.get("sales_commission_rub"),
            "advertising": pricing.get("advertising") or pricing.get("advertising_rub"),
            "cargo_loss": pricing.get("cargo_loss") or pricing.get("cargo_loss_rub"),
            "profit": pricing.get("profit") or pricing.get("profit_rub"),
            "gross_weight_g": scaled("weight_kg", 1000),
            "depth_mm": scaled("length_cm", 10),
            "width_mm": scaled("width_cm", 10),
            "height_mm": scaled("height_cm", 10),
            "category_display": inputs.get("subject_display"),
            "category_id": inputs.get("subject_id"),
            "warehouse_id": inputs.get("warehouse_id"),
            "stock": inputs.get("stock"),
            "status": "WB 商品、图片、价格及库存完成",
        }

    def _save_wb_job(self, state: dict):
        self.wb_job = state
        atomic_write_json(self.WB_JOB_PATH, state)
        root = getattr(self, "root", None)
        if root is not None and not getattr(self, "_closing", False):
            if threading.current_thread() is threading.main_thread():
                self._render_wb_job()
            else:
                root.after(0, self._render_wb_job)

    def _load_wb_job(self):
        state = load_json(self.WB_JOB_PATH, {})
        self.wb_job = state if isinstance(state, dict) and state.get("task_id") else {}
        if self.wb_job:
            self._apply_wb_job_inputs(self.wb_job)
        self._render_wb_job()

    def _apply_wb_job_inputs(self, state: dict):
        inputs = state.get("inputs") if isinstance(state.get("inputs"), dict) else {}
        for key, value in inputs.items():
            variable = self.vars.get("wb_" + key)
            if variable is not None and key not in {
                "description", "characteristic_definitions", "characteristic_values", "source_reference",
            }:
                variable.set(str(value or ""))
        self.wb_source_reference = dict(inputs.get("source_reference") or {})
        self.wb_description_text.delete("1.0", tk.END)
        self.wb_description_text.insert("1.0", str(inputs.get("description") or ""))
        self.wb_characteristics = list(inputs.get("characteristic_definitions") or [])
        self.wb_characteristic_values = dict(inputs.get("characteristic_values") or {})
        self._wb_refresh_characteristics_tree()
        self._wb_set_image_paths(state.get("image_paths") or [])
        self._wb_on_listing_mode_changed()
        self._wb_on_fulfillment_mode_changed()

    def _render_wb_job(self):
        if not hasattr(self, "wb_status_var"):
            return
        state = self.wb_job if isinstance(self.wb_job, dict) else {}
        stage = str(state.get("stage") or "new")
        progress = WbListingWorkflow.STAGES.get(stage, 0)
        self.wb_progress.configure(value=progress)
        if state:
            status = str(state.get("status") or "")
            message = str(state.get("message") or "")
            error = str(state.get("error") or "")
            self.wb_status_var.set(message or f"WB 任务：{stage} / {status}")
            identifiers = []
            if state.get("nm_id"):
                identifiers.append(f"nmID={state['nm_id']}")
            if state.get("chrt_id"):
                identifiers.append(f"chrtID={state['chrt_id']}")
            if error:
                identifiers.append("错误：" + error)
            self.wb_detail_var.set("；".join(identifiers))
            self.wb_submit_button.configure(
                text="已完成" if stage == "completed" else "继续当前任务",
            )
            if stage == "completed":
                self.wb_submit_button.state(["disabled"])
            elif self.vars.get("wb_token") and self.vars["wb_token"].get().strip():
                self.wb_submit_button.state(["!disabled"])
            barcode = str((state.get("inputs") or {}).get("barcode") or "")
            if barcode and not self.vars["wb_barcode"].get().strip():
                self.vars["wb_barcode"].set(barcode)
        else:
            self.wb_detail_var.set("")
            self.wb_submit_button.configure(text="开始上架")
            if self.vars.get("wb_token") and self.vars["wb_token"].get().strip():
                self.wb_status_var.set("WB API 已配置，等待录入")
                self.wb_submit_button.state(["!disabled"])
            else:
                self.wb_status_var.set("API 尚未配置")
                self.wb_submit_button.state(["disabled"])
        self._wb_on_listing_mode_changed()
        self._wb_on_fulfillment_mode_changed()

    def _reset_wb_job(self):
        if self.wb_job and not messagebox.askyesno(
            "重置 WB 任务",
            "这只会清除本机的 WB 断点记录，不会删除 WB 后台已经创建的商品。确认重置吗？",
        ):
            return
        self.wb_job = {}
        try:
            Path(self.WB_JOB_PATH).unlink(missing_ok=True)
        except OSError as error:
            messagebox.showerror("无法重置", str(error))
            return
        self.vars["wb_vendor_code"].set(
            f"WB-{time.strftime('%Y%m%d')}-{uuid.uuid4().hex[:6].upper()}"
        )
        self.vars["wb_barcode"].set("")
        self._wb_on_listing_mode_changed()
        self._render_wb_job()
