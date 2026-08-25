from __future__ import annotations

import json
import re
import threading
import tkinter as tk
from datetime import date, datetime, timedelta
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from typing import Any, Callable

from .api_bundle import (
    ApiBundleError, build_api_bundle, read_api_bundle, write_api_bundle,
)
from .config import APP_NAME, APP_VERSION, PALETTE
from .database import Database
from .exports import (
    ExportError, export_ai_docx, export_ai_pdf, export_replenishment_xlsx,
)
from .listing_ledger import (
    ListingLedgerError, find_listing_ledger, ledger_shop_names, read_ozon_cost_records,
    read_xlsx_rows,
)
from .listing_integration import (
    ListingToolError, find_listing_tool_executable, launch_listing_tool,
    sync_listing_shop_profile,
)
from .scheduler import install_weekly_task, remove_weekly_task
from .services import AppService, Credentials
from .shops import ShopProfile, ShopRegistry
from .wb_ui import WBWorkspace


class OzonAnalyticsApp(tk.Tk):
    def __init__(self, root_path: Path):
        super().__init__()
        self.root_path = root_path
        self.shop_registry = ShopRegistry(root_path / "data")
        self.active_shop = self.shop_registry.active()
        self.database = Database(self.shop_registry.database_path(self.active_shop))
        self.database.close_stale_running_logs()
        self.service = AppService(
            self.database, shop_name=self.active_shop.name, shop_kind=self.active_shop.kind,
        )
        self.current_page = "dashboard"
        self.nav_buttons: dict[str, tk.Button] = {}
        self.nav_accents: dict[str, tk.Frame] = {}
        self.nav_holders: dict[str, tk.Frame] = {}
        self.nav_category_buttons: dict[str, tk.Button] = {}
        self.nav_category_frames: dict[str, tk.Frame] = {}
        self.nav_category_labels: dict[str, str] = {}
        self.nav_page_categories: dict[str, str] = {}
        self.nav_expanded_category: str | None = None
        self.page_frames: dict[str, tk.Frame] = {}
        self._task_running = False
        self._cancel_event: threading.Event | None = None
        self._refresh_after_id: str | None = None
        self._search_after_ids: dict[str, str] = {}
        self._dirty_pages: set[str] = set()

        today = date.today()
        self.date_from_var = tk.StringVar(value=(today - timedelta(days=29)).isoformat())
        self.date_to_var = tk.StringVar(value=today.isoformat())
        self.date_preset_var = tk.StringVar(value="近30天")
        self.status_var = tk.StringVar(value="就绪")
        self.last_ai_result = ""
        self._currency_unit_labels: list[tk.Label] = []

        self.title(f"{APP_NAME} v{APP_VERSION}")
        self.geometry("1440x860")
        self.minsize(1180, 720)
        self.configure(bg=PALETTE.bg)
        self.option_add("*Font", ("Microsoft YaHei UI", 9))
        self.option_add("*Menu.background", PALETTE.panel)
        self.option_add("*Menu.foreground", PALETTE.text)
        self.option_add("*Menu.activeBackground", PALETTE.light_blue)
        self.option_add("*Menu.activeForeground", PALETTE.primary_dark)
        self.option_add("*TCombobox*Listbox.background", PALETTE.panel)
        self.option_add("*TCombobox*Listbox.foreground", PALETTE.text)
        self.option_add("*TCombobox*Listbox.selectBackground", PALETTE.light_blue)
        self.option_add("*TCombobox*Listbox.selectForeground", PALETTE.primary_dark)
        self._configure_styles()
        self._build_shell()
        self._build_pages()
        self._refresh_currency_display()
        self._apply_shop_page_access()
        self.show_page("dashboard")
        self._weekly_timer_id: str | None = None
        self._schedule_weekly_auto_check(1500)
        self._shipment_timer_id: str | None = None
        self._schedule_shipment_auto_check(5000)

    def _currency_code(self) -> str:
        """Return the display currency mandated by the active shop type."""
        return "CNY" if self.active_shop.kind == "cross_border" else "₽"

    def _display_amount(self, value: Any) -> float:
        """Convert Seller-native RUB amounts for a cross-border workspace display."""
        amount = float(value or 0)
        if self.active_shop.kind != "cross_border":
            return amount
        rate = self.database.rub_per_cny()
        return amount / rate if rate else amount

    def _fmt_amount(self, value: Any, *, include_currency: bool = False) -> str:
        text = _fmt_money(self._display_amount(value))
        return f"{text} {self._currency_code()}" if include_currency else text

    def _fmt_amount_decimal(self, value: Any, *, include_currency: bool = False) -> str:
        text = _fmt_decimal(self._display_amount(value))
        return f"{text} {self._currency_code()}" if include_currency else text

    def _refresh_currency_display(self) -> None:
        """Refresh shared RUB/CNY labels after switching the active shop."""
        unit = self._currency_code()
        for label in list(self._currency_unit_labels):
            try:
                label.configure(text=unit)
            except tk.TclError:
                pass
        if hasattr(self, "trend_mode_var"):
            self._set_trend_mode(self.trend_mode_var.get(), redraw=False)
        if hasattr(self, "order_detail_tree"):
            self.order_detail_tree.heading("price", text=f"订单金额 ({unit})")
            self.order_detail_tree.heading("fee_unit", text=f"基础配送/件 ({unit})")
            self.order_detail_tree.heading("fee_total", text=f"基础配送合计 ({unit})")

    def _refresh_shop_selector(self) -> None:
        shops = self.shop_registry.list()
        self._shops_by_display = {shop.display_name: shop for shop in shops}
        self._shop_values = tuple(self._shops_by_display)
        self._api_profiles_by_display = {shop.api_display_name: shop for shop in shops}
        self._api_profile_values = tuple(self._api_profiles_by_display)
        if hasattr(self, "shop_combo"):
            self.shop_combo.configure(values=self._shop_values)
            self.shop_var.set(self.active_shop.display_name)
        if hasattr(self, "api_profile_combo"):
            self.api_profile_combo.configure(values=self._api_profile_values)
            self.api_profile_var.set(self.active_shop.api_display_name)

    def _filter_shop_combo(self, event: tk.Event[Any]) -> None:
        if event.keysym in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        query = self.shop_var.get().strip().casefold()
        values = tuple(value for value in self._shop_values if query in value.casefold())
        self.shop_combo.configure(values=values or self._shop_values)
        if query:
            try:
                self.shop_combo.event_generate("<Down>")
            except tk.TclError:
                pass

    def _switch_shop_from_combo(self, _event: tk.Event[Any] | None = None) -> None:
        value = self.shop_var.get().strip()
        shop = self._shops_by_display.get(value)
        if shop is None:
            matches = [
                candidate for display, candidate in self._shops_by_display.items()
                if value.casefold() in display.casefold()
            ]
            if len(matches) == 1:
                shop = matches[0]
        if shop is None:
            messagebox.showwarning("选择店铺", "请选择列表中的一个店铺。", parent=self)
            self.shop_var.set(self.active_shop.display_name)
            return
        self.switch_shop(shop.id)

    def _filter_api_profile_combo(self, event: tk.Event[Any]) -> None:
        if event.keysym in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        query = self.api_profile_var.get().strip().casefold()
        values = tuple(
            value for value in self._api_profile_values if query in value.casefold()
        )
        self.api_profile_combo.configure(values=values or self._api_profile_values)
        if query:
            try:
                self.api_profile_combo.event_generate("<Down>")
            except tk.TclError:
                pass

    def _switch_api_profile_from_combo(
        self, _event: tk.Event[Any] | None = None,
    ) -> None:
        value = self.api_profile_var.get().strip()
        profile = self._api_profiles_by_display.get(value)
        if profile is None:
            matches = [
                candidate for display, candidate in self._api_profiles_by_display.items()
                if value.casefold() in display.casefold()
            ]
            if len(matches) == 1:
                profile = matches[0]
        if profile is None:
            messagebox.showwarning(
                "选择 API 配置", "请选择列表中的 API 配置。", parent=self,
            )
            self.api_profile_var.set(self.active_shop.api_display_name)
            return
        self.switch_shop(profile.id)
        self.show_page("settings")

    def switch_shop(self, shop_id: str, *, force_reload: bool = False) -> None:
        if self._task_running:
            messagebox.showwarning("任务进行中", "请等待当前同步或分析任务完成后再切换店铺。", parent=self)
            self.shop_var.set(self.active_shop.display_name)
            return
        if shop_id == self.active_shop.id and not force_reload:
            self.shop_var.set(self.active_shop.display_name)
            return
        self.active_shop = self.shop_registry.set_active(shop_id)
        self.database = Database(self.shop_registry.database_path(self.active_shop))
        self.database.close_stale_running_logs()
        self.service = AppService(
            self.database, shop_name=self.active_shop.name, shop_kind=self.active_shop.kind,
        )
        if hasattr(self, "credential_vars"):
            credentials = self.service.load_credentials()
            for key, variable in self.credential_vars.items():
                variable.set(str(getattr(credentials, key, "") or ""))
        if hasattr(self, "weekly_auto_var"):
            self.weekly_auto_var.set(
                self.database.get_setting("weekly_auto_sync_enabled", "1") != "0"
            )
        if hasattr(self, "feishu_weekly_auto_var"):
            self.feishu_weekly_auto_var.set(
                self.database.get_setting("feishu_weekly_auto_enabled", "1") != "0"
            )
        if hasattr(self, "shipment_tracking_auto_var"):
            self.shipment_tracking_auto_var.set(
                self.database.get_setting("shipment_tracking_auto_enabled", "1") != "0"
            )
        if hasattr(self, "exchange_rate_var"):
            self.exchange_rate_var.set(
                f"{self.database.rub_per_cny():.4f}".rstrip("0").rstrip(".")
            )
        if hasattr(self, "profit_tax_rate_var"):
            self.profit_tax_rate_var.set(self.database.get_setting("local_tax_rate", "6"))
            self.profit_payout_fee_rate_var.set(
                self.database.get_setting("local_payout_fee_rate", "1")
            )
            self.profit_delivery_default_var.set(
                self.database.get_setting("local_default_delivery_fee", "0")
            )
        if hasattr(self, "settings_shop_label"):
            self.settings_shop_label.configure(
                text=(
                    f"当前 API：{self.active_shop.resolved_api_name}  ｜  "
                    f"店铺：{self.active_shop.name}  ｜  类型：{self.active_shop.kind_label}  ｜  "
                    "独立 API / 独立数据库"
                )
            )
        if hasattr(self, "settings_type_badge"):
            self.settings_type_badge.configure(
                text=self.active_shop.kind_label,
                fg="#B54708" if self.active_shop.kind == "cross_border" else PALETTE.primary,
            )
        if hasattr(self, "settings_seller_title_label"):
            self.settings_seller_title_label.configure(
                text=f"{self.active_shop.kind_label} Seller API"
            )
        self._refresh_shop_selector()
        self.title(f"{APP_NAME} · {self.active_shop.name} · v{APP_VERSION}")
        self._apply_shop_page_access()
        self._refresh_currency_display()
        if self.active_shop.kind == "cross_border" and self.current_page in {
            "profit", "product_profit", "orders", "product_daily",
        }:
            self.show_page("cross_border")
        self.refresh_all()
        self.status_var.set(
            f"已切换到 {self.active_shop.display_name}；数据与 API 配置完全独立。"
        )

    def open_shop_manager(self, *, create_mode: bool = False) -> None:
        if self._task_running:
            messagebox.showwarning("任务进行中", "请等待当前任务结束后再管理店铺。", parent=self)
            return
        dialog = tk.Toplevel(self)
        dialog.title("新增 API 配置" if create_mode else "管理店铺 API 配置")
        dialog.geometry("1080x620")
        dialog.minsize(960, 560)
        dialog.configure(bg=PALETTE.bg)
        dialog.transient(self)

        panel = self._panel(dialog)
        panel.pack(fill="both", expand=True, padx=18, pady=18)
        self._panel_title(
            panel, "店铺 API 配置与数据空间",
            "先命名 API 配置并选择本土/跨境类型；新增后会立即切换到该配置填写密钥。",
        )
        name_var = tk.StringVar()
        api_name_var = tk.StringVar()
        kind_var = tk.StringVar(value="本土店")
        selected_id = tk.StringVar()

        editor = tk.Frame(
            panel, bg="#F8FAFC", highlightbackground=PALETTE.border,
            highlightthickness=1,
        )
        editor.pack(fill="x", padx=16, pady=(0, 9))
        tk.Label(
            editor, text="API 配置名称", bg="#F8FAFC", fg=PALETTE.muted,
        ).grid(row=0, column=0, sticky="w", padx=(14, 6), pady=(11, 5))
        api_name_entry = ttk.Entry(editor, width=25, textvariable=api_name_var)
        api_name_entry.grid(row=0, column=1, sticky="ew", padx=(0, 14), pady=(11, 5))
        tk.Label(
            editor, text="店铺名称", bg="#F8FAFC", fg=PALETTE.muted,
        ).grid(row=0, column=2, sticky="w", padx=(0, 6), pady=(11, 5))
        ttk.Entry(editor, width=23, textvariable=name_var).grid(
            row=0, column=3, sticky="ew", padx=(0, 14), pady=(11, 5),
        )
        tk.Label(editor, text="店铺类型", bg="#F8FAFC", fg=PALETTE.muted).grid(
            row=0, column=4, sticky="w", padx=(0, 6), pady=(11, 5),
        )
        kind_combo = ttk.Combobox(
            editor, width=10, state="normal", textvariable=kind_var,
            values=("本土店", "跨境店"),
        )
        kind_combo.grid(row=0, column=5, sticky="ew", padx=(0, 14), pady=(11, 5))
        tk.Label(
            editor,
            text="选择上方已有配置可修改名称和类型；类型改变时启用新的独立数据库，旧数据仍保留。",
            bg="#F8FAFC", fg="#B54708", font=("Microsoft YaHei UI", 8),
        ).grid(row=1, column=0, columnspan=6, sticky="w", padx=14, pady=(0, 10))
        editor.grid_columnconfigure(1, weight=1)
        editor.grid_columnconfigure(3, weight=1)

        actions = tk.Frame(panel, bg="#FFFFFF", height=46)
        actions.pack(fill="x", padx=16, pady=(0, 8))
        actions.pack_propagate(False)

        tree = self._tree(panel, [
            ("api_name", "API 配置名称", 220, "w"),
            ("name", "店铺名称", 210, "w"), ("kind", "店铺类型", 105, "w"),
            ("database", "独立数据库", 330, "w"),
        ])

        def load_rows() -> None:
            self._clear_tree(tree)
            for index, shop in enumerate(self.shop_registry.list()):
                tree.insert("", "end", iid=shop.id, values=(
                    shop.resolved_api_name,
                    ("● " if shop.id == self.active_shop.id else "") + shop.name,
                    shop.kind_label, shop.database_file,
                ), tags=("odd",) if index % 2 else ())

        def select_row(_event: tk.Event[Any] | None = None) -> None:
            selection = tree.selection()
            if not selection:
                return
            shop = self.shop_registry.get(selection[0])
            selected_id.set(shop.id)
            api_name_var.set(shop.resolved_api_name)
            name_var.set(shop.name)
            kind_var.set(shop.kind_label)

        def add_shop() -> None:
            if kind_var.get().strip() not in {"本土店", "跨境店"}:
                messagebox.showwarning(
                    "店铺类型无效", "店铺类型只能选择“本土店”或“跨境店”。",
                    parent=dialog,
                )
                return
            try:
                shop = self.shop_registry.add(
                    name_var.get(), "cross_border" if kind_var.get() == "跨境店" else "local",
                    api_name=api_name_var.get(),
                )
                Database(self.shop_registry.database_path(shop))
            except (ValueError, OSError) as error:
                messagebox.showerror("新增失败", str(error), parent=dialog)
                return
            dialog.destroy()
            self.switch_shop(shop.id)
            self.show_page("settings")
            self.status_var.set(
                f"已新增并切换到 {shop.api_display_name}；请填写并保存该店铺 API。"
            )

        def save_shop() -> None:
            if not selected_id.get():
                messagebox.showwarning("选择店铺", "请先选择需要修改的店铺。", parent=dialog)
                return
            if kind_var.get().strip() not in {"本土店", "跨境店"}:
                messagebox.showwarning(
                    "店铺类型无效", "店铺类型只能选择“本土店”或“跨境店”。",
                    parent=dialog,
                )
                return
            current = self.shop_registry.get(selected_id.get())
            requested_kind = "cross_border" if kind_var.get() == "跨境店" else "local"
            if requested_kind != current.kind and not messagebox.askyesno(
                "确认修改店铺类型",
                f"将“{current.name}”从 {current.kind_label} 改为 {kind_var.get()}。\n\n"
                "系统会为新类型启用一个全新的独立数据库和 API 凭证空间；"
                "原数据库会保留，但不会继续显示在该配置中。是否继续？",
                parent=dialog,
            ):
                return
            try:
                self.shop_registry.update(
                    selected_id.get(), name=name_var.get(),
                    kind=requested_kind,
                    api_name=api_name_var.get(),
                )
            except (ValueError, KeyError) as error:
                messagebox.showerror("保存失败", str(error), parent=dialog)
                return
            target = selected_id.get()
            dialog.destroy()
            self.switch_shop(target, force_reload=True)
            self.show_page("settings")
            self.status_var.set("API 配置名称、店铺名称和店铺类型已更新。")

        def activate_shop() -> None:
            if not selected_id.get():
                messagebox.showwarning("选择店铺", "请先选择店铺。", parent=dialog)
                return
            dialog.destroy()
            self.switch_shop(selected_id.get())

        def activate_and_configure() -> None:
            if not selected_id.get():
                messagebox.showwarning("选择店铺", "请先选择店铺。", parent=dialog)
                return
            target = selected_id.get()
            dialog.destroy()
            self.switch_shop(target)
            self.show_page("settings")

        def remove_shop() -> None:
            if not selected_id.get():
                return
            if selected_id.get() == self.active_shop.id:
                messagebox.showwarning(
                    "无法移除", "请先切换到其他店铺，再移除当前店铺。", parent=dialog,
                )
                return
            if not messagebox.askyesno(
                "移除店铺", "仅从列表移除店铺，数据库文件会保留以便恢复。是否继续？",
                parent=dialog,
            ):
                return
            try:
                self.shop_registry.remove(selected_id.get())
            except (ValueError, KeyError) as error:
                messagebox.showerror("移除失败", str(error), parent=dialog)
                return
            selected_id.set("")
            self._refresh_shop_selector()
            load_rows()

        self._button(actions, "+ 新增 API 配置", add_shop, compact=True).pack(side="left", padx=3, pady=4)
        self._button(actions, "保存名称与类型", save_shop, secondary=True, compact=True).pack(side="left", padx=3, pady=4)
        self._button(actions, "切换到此店", activate_shop, compact=True).pack(side="left", padx=3, pady=4)
        self._button(
            actions, "切换并填写 API", activate_and_configure,
            secondary=True, compact=True,
        ).pack(side="left", padx=3, pady=4)
        self._button(actions, "移除", remove_shop, secondary=True, compact=True).pack(side="right", pady=4)
        tree.bind("<<TreeviewSelect>>", select_row)
        tree.bind("<Double-1>", lambda _event: activate_shop())
        load_rows()
        if create_mode:
            api_name_entry.focus_set()

    def _configure_styles(self) -> None:
        style = ttk.Style(self)
        try:
            style.theme_use("clam")
        except tk.TclError:
            pass

        style.configure(
            "Treeview", background=PALETTE.panel, fieldbackground=PALETTE.panel,
            foreground=PALETTE.text, rowheight=42, font=("Microsoft YaHei UI", 9),
            borderwidth=0, relief="flat",
        )
        style.configure(
            "Treeview.Heading", background="#F8FAFD", foreground="#566176",
            font=("Microsoft YaHei UI", 9, "bold"), relief="flat",
            borderwidth=0, padding=(12, 12),
        )
        style.map(
            "Treeview", background=[("selected", PALETTE.light_blue)],
            foreground=[("selected", PALETTE.primary_dark)],
        )
        style.map("Treeview.Heading", background=[("active", "#F0F3F9")])
        style.configure(
            "TEntry", padding=(11, 9), fieldbackground=PALETTE.panel,
            foreground=PALETTE.text, bordercolor=PALETTE.border, lightcolor=PALETTE.border,
            darkcolor=PALETTE.border, insertcolor=PALETTE.primary,
        )
        style.map(
            "TEntry", bordercolor=[("focus", PALETTE.primary)],
            lightcolor=[("focus", PALETTE.primary)],
            darkcolor=[("focus", PALETTE.primary)],
        )
        style.configure(
            "TCombobox", padding=(11, 9), fieldbackground=PALETTE.panel,
            foreground=PALETTE.text, bordercolor=PALETTE.border,
            lightcolor=PALETTE.border, darkcolor=PALETTE.border,
            arrowcolor=PALETTE.muted,
        )
        style.map(
            "TCombobox", fieldbackground=[("readonly", PALETTE.panel)],
            selectbackground=[("readonly", PALETTE.panel)],
            selectforeground=[("readonly", PALETTE.text)],
            bordercolor=[("focus", PALETTE.primary)],
            arrowcolor=[("active", PALETTE.primary)],
        )
        style.configure(
            "Vertical.TScrollbar", background="#C9D1E0", troughcolor=PALETTE.panel_alt,
            bordercolor=PALETTE.panel_alt, arrowcolor=PALETTE.muted, width=10,
        )
        style.configure(
            "Horizontal.TScrollbar", background="#C9D1E0", troughcolor=PALETTE.panel_alt,
            bordercolor=PALETTE.panel_alt, arrowcolor=PALETTE.muted, width=10,
        )
        style.configure(
            "Horizontal.TProgressbar", background=PALETTE.primary,
            troughcolor="#EAECF0", borderwidth=0, thickness=6,
        )

    def save_exchange_rate(self) -> None:
        try:
            rate = self.database.save_rub_per_cny(float(self.exchange_rate_var.get()))
        except (TypeError, ValueError) as error:
            messagebox.showerror("汇率错误", str(error), parent=self)
            return
        self.exchange_rate_var.set(f"{rate:.4f}".rstrip("0").rstrip("."))
        self.refresh_costs()
        self.refresh_profit()
        if self.current_page == "cross_border":
            self.refresh_cross_border()
        self.status_var.set(
            f"{self.active_shop.name} 汇率已更新为 1 CNY = {rate:g} RUB；相关成本与利润已重算。"
        )

    def refresh_costs(self) -> None:
        if not hasattr(self, "cost_tree"):
            return
        rows = self.database.product_cost_rows(self.cost_search_var.get())
        self._clear_tree(self.cost_tree)
        self._cost_rows_by_iid = {}
        for index, row in enumerate(rows):
            sku = str(row.get("sku") or "")
            if not sku:
                continue
            self._cost_rows_by_iid[sku] = row
            dimensions = "×".join(
                "—" if row.get(key) is None else f"{float(row[key]):g}"
                for key in ("length_cm", "width_cm", "height_cm")
            )
            self.cost_tree.insert("", "end", iid=sku, values=(
                row.get("offer_id") or "—", sku,
                "—" if row.get("unit_cost_cny") is None else f"{float(row['unit_cost_cny']):.2f}",
                "—" if row.get("first_mile_cost") is None else _fmt_money(row["first_mile_cost"]),
                f"{float(row['rub_per_cny']):.4f}".rstrip("0").rstrip("."),
                "—" if row.get("unit_cost_rub") is None else _fmt_money(row["unit_cost_rub"]),
                dimensions,
                "—" if row.get("weight_kg") is None else f"{float(row['weight_kg']):g}",
                row.get("note") or "",
            ), tags=("odd",) if index % 2 else ())
        all_rows = self.database.product_cost_rows()
        configured = sum(1 for row in all_rows if row.get("unit_cost_cny") is not None)
        self.cost_summary_labels["products"].configure(text=_fmt_int(len(all_rows)))
        self.cost_summary_labels["configured"].configure(text=_fmt_int(configured))
        self.cost_summary_labels["missing"].configure(text=_fmt_int(len(all_rows) - configured))
        self.cost_summary_labels["rate"].configure(text=f"{self.database.rub_per_cny():g}")

    def refresh_cross_border(self, reload_data: bool = True) -> None:
        if not hasattr(self, "cross_border_tree") or self.current_page != "cross_border":
            return
        is_cross_border = self.active_shop.kind == "cross_border"
        for name in ("cross_border_sync_button", "cross_border_ledger_button",
                     "cross_border_ledger_sync_button", "cross_border_listing_button",
                     "cross_border_tool_button"):
            button = getattr(self, name, None)
            if button is not None:
                button.configure(state="normal" if is_cross_border else "disabled")
        if not is_cross_border:
            self._cross_border_full_rows = []
            self._cross_border_rows_by_iid = {}
            self._clear_tree(self.cross_border_tree)
            self._clear_tree(self.cross_border_week_tree)
            for label in self.cross_border_summary_labels.values():
                label.configure(text="—")
            self.cross_border_hint.configure(
                text="当前是本土店：本页已停止读取本土数据库。请切换或新增跨境店。",
                fg="#D92D20",
            )
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        ledger_note = ""
        if reload_data:
            try:
                imported = self._sync_listing_ledger_costs(force=False, show_message=False)
                if imported and int(imported.get("updated_skus") or 0):
                    ledger_note = f" · 台账更新 {imported['updated_skus']} 个 SKU"
            except (ListingLedgerError, OSError, ValueError):
                # Configuration and manual sync show actionable errors; report
                # refresh itself must stay usable when the external file is busy.
                pass
            self._cross_border_full_rows = self.database.cross_border_profit_rows(
                date_from, date_to,
            )
            self._cross_border_summary = self.database.cross_border_period_summary(
                date_from, date_to, self._cross_border_full_rows,
            )
            self._cross_border_weeks = self.database.cross_border_weekly_summary(
                date_from, date_to,
            )
        rows = list(getattr(self, "_cross_border_full_rows", []))
        query = self.cross_border_search_var.get().strip().casefold()
        if query:
            rows = [
                row for row in rows
                if query in f"{row.get('offer_id', '')} {row.get('sku', '')}".casefold()
            ]
        scheme_variable = getattr(self, "cross_border_scheme_var", None)
        scheme_filter = scheme_variable.get() if scheme_variable is not None else "全部履约"
        scheme_key = {
            "FBP": "fbp_orders", "FBS / realFBS": "rfbs_orders",
            "WHD": "whd_orders", "其它 / 未识别": "other_fulfillment_orders",
        }.get(scheme_filter)
        if scheme_key:
            rows = [row for row in rows if int(row.get(scheme_key) or 0) > 0]
        summary = getattr(self, "_cross_border_summary", None)
        if summary is None:
            return
        self._clear_tree(self.cross_border_tree)
        self._cross_border_rows_by_iid = {}
        for index, row in enumerate(rows):
            sku = str(row.get("sku") or "")
            if not sku:
                continue
            self._cross_border_rows_by_iid[sku] = row
            freight_text = "—"
            if row.get("weight_kg") is not None and row.get("freight_unit_cny") is None:
                freight_text = "超出区间"
            elif row.get("freight_unit_cny") is not None:
                freight_text = f"{float(row['freight_unit_cny']):.2f}"
            self.cross_border_tree.insert("", "end", iid=sku, values=(
                row.get("offer_id") or "—", sku, _fmt_int(row["orders"]),
                _fmt_int(row.get("fulfillment_orders") or 0),
                _fmt_int(row.get("fbp_orders") or 0),
                _fmt_int(row.get("rfbs_orders") or 0),
                _fmt_int(row.get("whd_orders") or 0),
                _fmt_money(row["revenue_cny"]), f"{float(row['selling_price_cny']):.2f}",
                "—" if row.get("purchase_cost_cny") is None else f"{float(row['purchase_cost_cny']):.2f}",
                "—" if row.get("weight_kg") is None else f"{float(row['weight_kg']):g}",
                freight_text,
                "—" if row.get("purchase_cny_total") is None else _fmt_money(row["purchase_cny_total"]),
                "—" if row.get("freight_cny_total") is None else _fmt_money(row["freight_cny_total"]),
                "—" if row.get("estimated_platform_fees_cny") is None
                else _fmt_money(row["estimated_platform_fees_cny"]),
                "—" if row.get("contribution_before_shop_ads_cny") is None
                else _fmt_money(row["contribution_before_shop_ads_cny"]),
                _fmt_money(row["finance_base_cny"]) if int(row.get("finance_rows") or 0) else "—",
                row.get("realtime_profit_formula_cny") or "数据不足",
            ), tags=("odd",) if index % 2 else ())
        self.cross_border_summary_labels["revenue"].configure(text=_fmt_money(summary["revenue_cny"]))
        self.cross_border_summary_labels["orders"].configure(text=_fmt_int(summary["orders"]))
        self.cross_border_summary_labels["ad_spend"].configure(text=_fmt_money(summary["ad_spend_cny"]))
        self.cross_border_summary_labels["other_expenses"].configure(
            text=_fmt_money(summary["estimated_platform_fees_cny"])
        )
        self.cross_border_summary_labels["cost"].configure(
            text=_fmt_money(summary["purchase_and_freight_cost_cny"])
            if getattr(self, "_cross_border_full_rows", []) else "—"
        )
        self.cross_border_summary_labels["profit"].configure(
            text=_fmt_money(summary["profit_cny"]) if summary["profit_cny"] is not None else "—"
        )
        commission_rate = summary.get("estimated_commission_rate")
        acquiring_rate = summary.get("estimated_acquiring_rate")
        fee_text = (
            f"佣金 {float(commission_rate) * 100:.2f}% / 收单 {float(acquiring_rate) * 100:.2f}%"
            if commission_rate is not None and acquiring_rate is not None
            else "佣金/收单历史费率不足"
        )
        self.cross_border_hint.configure(
            text=f"{self.active_shop.name} · 跨境店独立数据 · 1 CNY={summary['rub_per_cny']:g} RUB · "
            f"实时预估费率：{fee_text}（样本截至 {summary.get('fee_history_through', '全部缓存')}） · "
            f"{summary['sold_skus']} 个出单 SKU / "
            f"{summary['missing_cost_skus']} 个待补采购成本、重量或费率 · "
            f"缓存履约单 FBP {summary['fbp_orders']} / FBS {summary['rfbs_orders']} / "
            f"WHD {summary['whd_orders']}"
            f"{ledger_note}",
            fg=PALETTE.muted,
        )
        self._clear_tree(self.cross_border_week_tree)
        for index, week in enumerate(getattr(self, "_cross_border_weeks", [])):
            self.cross_border_week_tree.insert("", "end", values=(
                f"{week['date_from']} 至 {week['date_to']}", _fmt_int(week["orders"]),
                _fmt_money(week["revenue_cny"]), _fmt_money(week["ad_spend_cny"]),
                _fmt_money(week["estimated_platform_fees_cny"]),
                _fmt_money(week["purchase_and_freight_cost_cny"]),
                _fmt_money(week["profit_cny"]) if week["profit_cny"] is not None else "—",
                _fmt_money(week["settled_finance_net_cny"])
                if week["finance_available"] else "—",
            ), tags=("odd",) if index % 2 else ())

    @staticmethod
    def _default_listing_tool_root() -> Path:
        return Path(r"D:\OZON 跟卖软件\新建文件夹\新建文件夹 (2)")

    def _listing_ledger_path(self) -> Path | None:
        ledger_text = self.database.get_setting("listing_ledger_path", "").strip()
        if ledger_text:
            return Path(ledger_text)
        return find_listing_ledger(self._default_listing_tool_root())

    def _listing_tool_executable(self) -> Path | None:
        configured = self.database.get_setting("listing_tool_executable", "").strip()
        if configured and Path(configured).is_file():
            return Path(configured)
        return find_listing_tool_executable(
            self._default_listing_tool_root(), self.root_path,
        )

    def _listing_tool_data_dir(self) -> Path:
        configured = self.database.get_setting("listing_tool_data_dir", "").strip()
        if configured:
            return Path(configured)
        ledger = self._listing_ledger_path()
        if ledger is not None:
            return ledger.parent
        source_data = self._default_listing_tool_root() / "RFBS上品工具"
        if source_data.is_dir():
            return source_data
        return self.root_path / "data" / "listing_tool"

    def open_listing_tool(self) -> None:
        """Select the active cross-border API in the listing tool, then launch it."""
        if self.active_shop.kind != "cross_border":
            messagebox.showwarning(
                "请切换店铺",
                "上品工具只接收跨境店 Seller API。请先切换到跨境 API 配置。",
                parent=self,
            )
            return
        executable = self._listing_tool_executable()
        if executable is None:
            messagebox.showwarning(
                "未找到上品工具",
                "请在“连接 / 更换台账”中选择 Ozon_RFBS上品工具.exe。",
                parent=self,
            )
            return
        credentials = self.service.load_credentials()
        data_dir = self._listing_tool_data_dir()
        try:
            sync_listing_shop_profile(
                data_dir / "config.json",
                profile_id=f"analytics-{self.active_shop.id}",
                name=self.active_shop.name,
                client_id=credentials.seller_client_id,
                api_key=credentials.seller_api_key,
            )
            pid = launch_listing_tool(executable, data_dir)
        except ListingToolError as error:
            messagebox.showerror("上品工具启动失败", str(error), parent=self)
            return
        self.database.save_settings({
            "listing_tool_executable": str(executable),
            "listing_tool_data_dir": str(data_dir),
        })
        self.status_var.set(
            f"已打开上品工具（PID {pid}）；已自动切换为 {self.active_shop.api_display_name}。"
        )

    def _sync_listing_ledger_costs(
        self, *, force: bool, show_message: bool,
    ) -> dict[str, Any] | None:
        if self.active_shop.kind != "cross_border":
            if show_message:
                messagebox.showwarning(
                    "店铺类型不符", "上品台账只能同步到当前跨境店，不能写入本土店。", parent=self,
                )
            return None
        ledger = self._listing_ledger_path()
        ledger_shop = self.database.get_setting("listing_ledger_shop_name", "").strip()
        if ledger is None or not ledger.is_file() or not ledger_shop:
            if show_message:
                self.configure_listing_ledger()
            return None
        mtime = str(ledger.stat().st_mtime_ns)
        if not force and self.database.get_setting("listing_ledger_mtime_ns", "") == mtime:
            return {"updated_skus": 0, "unchanged": True}
        records = read_ozon_cost_records(ledger, ledger_shop)
        result = self.database.import_listing_ledger_costs(
            records, source_label=f"上品台账：{ledger_shop}",
        )
        self.database.save_settings({
            "listing_ledger_mtime_ns": mtime,
            "listing_ledger_last_sync": datetime.now().isoformat(timespec="seconds"),
        })
        if show_message:
            unmatched = result.get("unmatched_offers") or ()
            messagebox.showinfo(
                "台账同步完成",
                f"台账店铺：{ledger_shop}\n"
                f"读取 {result['ledger_rows']} 行，匹配 {result['matched_offers']} 个货号，"
                f"更新 {result['updated_skus']} 个当前店铺 SKU。\n"
                f"尚未在本店 API 数据中找到：{len(unmatched)} 个货号。",
                parent=self,
            )
        return result

    def sync_listing_ledger_costs(self) -> None:
        try:
            result = self._sync_listing_ledger_costs(force=True, show_message=True)
        except (ListingLedgerError, OSError, ValueError) as error:
            messagebox.showerror("台账同步失败", str(error), parent=self)
            return
        if result is not None:
            self.refresh_costs()
            self.refresh_cross_border(reload_data=True)
            self.refresh_listing_page(reload_data=True)

    def configure_listing_ledger(self) -> None:
        if self.active_shop.kind != "cross_border":
            messagebox.showwarning(
                "请切换店铺", "请先切换或新增跨境店，再连接该店对应的上品台账。", parent=self,
            )
            return
        dialog = tk.Toplevel(self)
        dialog.title("连接上品产品台账")
        dialog.geometry("820x390")
        dialog.minsize(740, 360)
        dialog.configure(bg=PALETTE.bg)
        dialog.transient(self)
        ledger = self._listing_ledger_path()
        ledger_var = tk.StringVar(value=str(ledger or ""))
        tool_var = tk.StringVar(value=str(self._listing_tool_executable() or ""))
        shop_var = tk.StringVar(
            value=self.database.get_setting("listing_ledger_shop_name", "")
        )
        panel = self._panel(dialog)
        panel.pack(fill="both", expand=True, padx=20, pady=20)
        self._panel_title(
            panel, "产品台账成本联动",
            "台账用于联动成本；启动上品工具时会自动选中当前跨境店 Seller API。",
        )

        form = tk.Frame(panel, bg="#FFFFFF")
        form.pack(fill="x", padx=18, pady=(0, 10))

        def path_row(row: int, title: str, variable: tk.StringVar, command: Callable[[], None]) -> None:
            tk.Label(form, text=title, bg="#FFFFFF", fg=PALETTE.muted).grid(
                row=row, column=0, sticky="w", pady=6,
            )
            ttk.Entry(form, textvariable=variable).grid(
                row=row, column=1, sticky="ew", padx=8, pady=6,
            )
            self._button(form, "选择", command, secondary=True, compact=True).grid(
                row=row, column=2, pady=6,
            )

        def choose_ledger() -> None:
            path = filedialog.askopenfilename(
                parent=dialog, title="选择产品台账",
                filetypes=[("Excel 产品台账", "*.xlsx")],
                initialdir=str(Path(ledger_var.get()).parent) if ledger_var.get() else None,
            )
            if path:
                ledger_var.set(path)
                load_shop_names()

        def choose_tool() -> None:
            path = filedialog.askopenfilename(
                parent=dialog, title="选择上品工具",
                filetypes=[("上品工具", "*.exe")],
                initialdir=str(Path(tool_var.get()).parent) if tool_var.get() else None,
            )
            if path:
                tool_var.set(path)

        path_row(0, "产品台账", ledger_var, choose_ledger)
        path_row(1, "上品工具", tool_var, choose_tool)
        tk.Label(form, text="台账店铺", bg="#FFFFFF", fg=PALETTE.muted).grid(
            row=2, column=0, sticky="w", pady=6,
        )
        shop_combo = ttk.Combobox(form, state="normal", textvariable=shop_var)
        shop_combo.grid(row=2, column=1, sticky="ew", padx=8, pady=6)
        form.grid_columnconfigure(1, weight=1)

        def load_shop_names() -> None:
            path = Path(ledger_var.get().strip())
            if not path.is_file():
                shop_combo.configure(values=())
                return
            try:
                names = tuple(ledger_shop_names(path))
            except ListingLedgerError as error:
                messagebox.showerror("读取失败", str(error), parent=dialog)
                return
            shop_combo.configure(values=names)
            if names and shop_var.get() not in names:
                shop_var.set(names[0])

        def save() -> None:
            ledger_path = Path(ledger_var.get().strip())
            if not ledger_path.is_file():
                messagebox.showwarning("台账无效", "请选择实际存在的产品台账.xlsx。", parent=dialog)
                return
            names = ledger_shop_names(ledger_path)
            if shop_var.get().strip() not in names:
                messagebox.showwarning("店铺无效", "请选择台账内真实存在的上品店铺。", parent=dialog)
                return
            tool_path = Path(tool_var.get().strip())
            if tool_var.get().strip() and not tool_path.is_file():
                messagebox.showwarning("工具无效", "请选择实际存在的上品工具 EXE。", parent=dialog)
                return
            self.database.save_settings({
                "listing_ledger_path": str(ledger_path),
                "listing_ledger_shop_name": shop_var.get().strip(),
                "listing_ledger_mtime_ns": "",
                "listing_tool_executable": str(tool_path) if tool_path.is_file() else "",
                "listing_tool_data_dir": str(ledger_path.parent),
            })
            dialog.destroy()
            self.sync_listing_ledger_costs()
            self.refresh_listing_page(reload_data=True)

        actions = tk.Frame(panel, bg="#FFFFFF")
        actions.pack(fill="x", padx=18, pady=(4, 14))
        self._button(actions, "保存并立即同步", save, compact=True).pack(side="right")
        self._button(actions, "读取台账店铺", load_shop_names, secondary=True, compact=True).pack(
            side="right", padx=7,
        )
        load_shop_names()

    def refresh_listing_page(self, *, reload_data: bool = False) -> None:
        if not hasattr(self, "listing_ledger_tree"):
            return
        ledger = self._listing_ledger_path()
        selected_shop = self.database.get_setting("listing_ledger_shop_name", "").strip()
        ledger_exists = ledger is not None and ledger.is_file()
        self.listing_source_label.configure(
            text=f"产品台账：{ledger if ledger_exists else '尚未连接'}",
            fg=PALETTE.muted if ledger_exists else PALETTE.red,
        )
        allowed = self.active_shop.kind == "cross_border"
        for button in (
            self.listing_connect_button, self.listing_sync_cost_button,
            self.listing_open_tool_button,
        ):
            button.configure(state="normal" if allowed else "disabled")
        self.listing_guard_label.configure(
            text=(
                f"当前店铺：{self.active_shop.api_display_name}"
                if allowed else
                f"当前是 {self.active_shop.kind_label}：台账成本不会写入本土店，请切换跨境店。"
            ),
            fg=PALETTE.green if allowed else PALETTE.red,
        )
        if reload_data or not hasattr(self, "_listing_ledger_cache"):
            if not ledger_exists:
                self._listing_ledger_cache = []
            else:
                try:
                    all_rows = read_xlsx_rows(ledger)
                    self._listing_ledger_cache = [
                        row for row in all_rows
                        if str(row.get("平台") or "Ozon").strip().casefold().startswith("ozon")
                        and (
                            not selected_shop
                            or str(row.get("上品店铺") or "").strip() == selected_shop
                        )
                    ][:3000]
                except ListingLedgerError as error:
                    self._listing_ledger_cache = []
                    self.status_var.set(str(error))
        try:
            shops = tuple(ledger_shop_names(ledger)) if ledger_exists else ()
        except ListingLedgerError as error:
            shops = ()
            self.status_var.set(str(error))
        self.listing_metric_labels["shops"].configure(text=_fmt_int(len(shops)))
        self.listing_metric_labels["rows"].configure(
            text=_fmt_int(len(self._listing_ledger_cache))
        )
        query = self.listing_search_var.get().strip().casefold()
        rows = [
            row for row in self._listing_ledger_cache
            if not query or query in " ".join(str(value or "") for value in row.values()).casefold()
        ]
        self.listing_metric_labels["visible"].configure(text=_fmt_int(len(rows)))
        self._clear_tree(self.listing_ledger_tree)
        for index, row in enumerate(rows[:1000]):
            dimensions = "×".join(
                str(row.get(name) or "—")
                for name in ("包装长度(mm)", "包装宽度(mm)", "包装高度(mm)")
            )
            self.listing_ledger_tree.insert("", "end", values=(
                row.get("上品店铺") or "—", row.get("平台") or "—",
                row.get("货号") or "—", row.get("Ozon商品ID") or row.get("WB商品ID(nmID)") or "—",
                row.get("采购成本") if row.get("采购成本") not in (None, "") else "—",
                row.get("包装毛重(g)") if row.get("包装毛重(g)") not in (None, "") else "—",
                dimensions, row.get("状态") or "—", row.get("更新时间") or "—",
            ), tags=("odd",) if index % 2 else ())
        self.listing_shop_label.configure(
            text=f"当前读取：{selected_shop or '尚未选择台账店铺'}"
        )

    def _build_shell(self) -> None:
        sidebar = tk.Frame(
            self, bg=PALETTE.sidebar, width=258,
            highlightbackground=PALETTE.border, highlightthickness=1,
        )
        sidebar.pack(side="left", fill="y")
        sidebar.pack_propagate(False)

        logo = tk.Frame(sidebar, bg=PALETTE.sidebar, height=88)
        logo.pack(fill="x")
        logo.pack_propagate(False)
        brand = tk.Frame(logo, bg=PALETTE.sidebar)
        brand.pack(anchor="w", padx=22, pady=(18, 0))
        badge = tk.Frame(brand, bg=PALETTE.primary, width=40, height=40)
        badge.pack(side="left")
        badge.pack_propagate(False)
        tk.Label(
            badge, text="O", font=("Segoe UI", 21, "bold"), fg="#FFFFFF",
            bg=PALETTE.primary,
        ).place(relx=0.5, rely=0.48, anchor="center")
        brand_text = tk.Frame(brand, bg=PALETTE.sidebar)
        brand_text.pack(side="left", padx=(11, 0))
        tk.Label(
            brand_text, text="OZON", font=("Segoe UI", 18, "bold"), fg=PALETTE.text,
            bg=PALETTE.sidebar,
        ).pack(anchor="w")
        tk.Label(
            brand_text, text="SELLER ANALYTICS", font=("Segoe UI", 7, "bold"),
            fg=PALETTE.sidebar_muted, bg=PALETTE.sidebar,
        ).pack(anchor="w")

        shop_box = tk.Frame(sidebar, bg=PALETTE.sidebar)
        shop_box.pack(fill="x", padx=15, pady=(0, 7))
        self.shop_var = tk.StringVar(value=self.active_shop.display_name)
        self.shop_combo = ttk.Combobox(
            shop_box, state="normal", width=22, textvariable=self.shop_var,
        )
        self.shop_combo.pack(fill="x")
        self.shop_combo.bind("<<ComboboxSelected>>", self._switch_shop_from_combo)
        self.shop_combo.bind("<Return>", self._switch_shop_from_combo)
        self.shop_combo.bind("<KeyRelease>", self._filter_shop_combo)
        tk.Button(
            shop_box, text="管理店铺 / API 配置", command=self.open_shop_manager,
            relief="flat", bd=0, cursor="hand2", bg=PALETTE.sidebar_hover,
            fg=PALETTE.sidebar_text, activebackground=PALETTE.sidebar_active,
            activeforeground=PALETTE.primary_dark, font=("Microsoft YaHei UI", 8), pady=6,
        ).pack(fill="x", pady=(4, 0))
        tk.Button(
            shop_box, text="打开 WB 独立工作台", command=self.open_wb_workspace,
            relief="flat", bd=0, cursor="hand2", bg="#F2EEFF", fg="#6941C6",
            activebackground="#E9E2FF", activeforeground="#53389E",
            font=("Microsoft YaHei UI", 8, "bold"), pady=6,
        ).pack(fill="x", pady=(4, 0))
        self._refresh_shop_selector()

        tk.Label(
            sidebar, text="工作台", font=("Microsoft YaHei UI", 8, "bold"),
            fg=PALETTE.sidebar_muted, bg=PALETTE.sidebar,
        ).pack(anchor="w", padx=25, pady=(2, 4))

        nav_categories = [
            ("analysis", "经营与分析", [
                ("dashboard", "▦  经营总览"),
                ("advertising", "◉  广告分析"),
                ("sales", "▤  商品销量"),
                ("product_daily", "▦  每日产品经营"),
                ("series", "▦  产品系列分析"),
                ("ai", "✦  AI 分析"),
            ]),
            ("profit_cost", "利润与成本", [
                ("profit", "▥  月度盈亏表"),
                ("product_profit", "▥  产品利润表"),
                ("costs", "▤  产品成本"),
                ("cross_border", "▦  跨境店利润"),
            ]),
            ("supply_chain", "订单与供应链", [
                ("orders", "▤  订单明细"),
                ("inventory", "▣  库存管理"),
                ("supply", "▣  约仓计划"),
                ("shipment_tracking", "◆  发货跟踪"),
            ]),
            ("cross_border_ops", "跨境业务", [
                ("listing", "▣  跨境上品"),
            ]),
            ("reports", "报表与协作", [
                ("weekly", "▧  周报"),
                ("feishu", "◆  飞书同步"),
            ]),
            ("system", "数据与设置", [
                ("sync", "↻  数据中心"),
                ("settings", "⚙  API 配置中心"),
            ]),
        ]
        for category_key, category_label, nav_items in nav_categories:
            section = tk.Frame(sidebar, bg=PALETTE.sidebar)
            section.pack(fill="x", padx=12, pady=1)
            category_button = tk.Button(
                section, text=f"▸  {category_label}", anchor="w", relief="flat",
                bd=0, cursor="hand2", font=("Microsoft YaHei UI", 9, "bold"),
                fg=PALETTE.sidebar_muted, bg=PALETTE.sidebar,
                activeforeground=PALETTE.primary_dark, activebackground=PALETTE.sidebar_hover,
                padx=12, pady=8,
                command=lambda category=category_key: self._toggle_nav_category(category),
            )
            category_button.pack(fill="x")
            group = tk.Frame(section, bg=PALETTE.sidebar)
            self.nav_category_buttons[category_key] = category_button
            self.nav_category_frames[category_key] = group
            self.nav_category_labels[category_key] = category_label
            for key, label in nav_items:
                self.nav_page_categories[key] = category_key
                holder = tk.Frame(group, bg=PALETTE.sidebar, height=38)
                holder.pack(fill="x", pady=2)
                holder.pack_propagate(False)
                accent = tk.Frame(holder, bg=PALETTE.sidebar, width=3)
                accent.pack(side="left", fill="y")
                button = tk.Button(
                    holder, text=label, anchor="w", relief="flat", bd=0,
                    cursor="hand2", font=("Microsoft YaHei UI", 9),
                    fg=PALETTE.sidebar_text, bg=PALETTE.sidebar,
                    activeforeground=PALETTE.primary_dark, activebackground=PALETTE.sidebar_hover,
                    padx=25, pady=7, command=lambda page=key: self.show_page(page),
                )
                button.pack(side="left", fill="both", expand=True)
                button.bind("<Enter>", lambda _event, page=key: self._nav_hover(page, True))
                button.bind("<Leave>", lambda _event, page=key: self._nav_hover(page, False))
                self.nav_buttons[key] = button
                self.nav_accents[key] = accent
                self.nav_holders[key] = holder
        self._toggle_nav_category("analysis", force_open=True)

        local_card = tk.Frame(
            sidebar, bg="#ECFDF5", highlightbackground="#D1FAE5",
            highlightthickness=1,
        )
        local_card.pack(side="bottom", fill="x", padx=18, pady=10)
        tk.Label(
            local_card, text="●  本地安全模式", font=("Microsoft YaHei UI", 8, "bold"),
            fg="#047857", bg="#ECFDF5",
        ).pack(anchor="w", padx=13, pady=(11, 2))
        tk.Label(
            local_card, text=f"v{APP_VERSION}  ·  数据仅保存在本机", justify="left",
            font=("Microsoft YaHei UI", 7), fg="#6B7C72", bg="#ECFDF5",
        ).pack(anchor="w", padx=13, pady=(0, 11))

        right = tk.Frame(self, bg=PALETTE.bg)
        right.pack(side="left", fill="both", expand=True)

        self.content = tk.Frame(right, bg=PALETTE.bg)
        self.content.pack(fill="both", expand=True)

        status_bar = tk.Frame(
            right, bg=PALETTE.panel, height=40,
            highlightbackground=PALETTE.border, highlightthickness=1,
        )
        status_bar.pack(side="bottom", fill="x")
        tk.Label(
            status_bar, text="●", bg=PALETTE.panel, fg=PALETTE.green,
            font=("Segoe UI", 8),
        ).pack(side="left", padx=(18, 7))
        tk.Label(
            status_bar, textvariable=self.status_var, bg=PALETTE.panel, fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        ).pack(side="left")
        tk.Label(
            status_bar, text="LOCAL DATABASE", bg=PALETTE.panel, fg=PALETTE.sidebar_muted,
            font=("Segoe UI", 7, "bold"),
        ).pack(side="right", padx=(0, 18))
        self.progress = ttk.Progressbar(status_bar, mode="indeterminate", length=140)
        self.progress.pack(side="right", padx=14, pady=12)

    def open_wb_workspace(self) -> None:
        window = getattr(self, "_wb_workspace", None)
        if window is None or not window.winfo_exists():
            window = WBWorkspace(self)
            self._wb_workspace = window
        else:
            window.deiconify()
        window.lift()
        window.focus_force()

    def _build_pages(self) -> None:
        for key in (
            "dashboard", "advertising", "sales", "ai", "profit", "product_profit",
            "orders", "product_daily", "series", "weekly", "costs", "cross_border", "listing",
            "inventory", "supply", "feishu", "shipment_tracking", "sync", "settings",
        ):
            frame = tk.Frame(self.content, bg=PALETTE.bg)
            frame.place(relx=0, rely=0, relwidth=1, relheight=1)
            self.page_frames[key] = frame
        self._build_dashboard_page(self.page_frames["dashboard"])
        self._build_advertising_page(self.page_frames["advertising"])
        self._build_sales_page(self.page_frames["sales"])
        self._build_ai_page(self.page_frames["ai"])
        self._build_profit_page(self.page_frames["profit"])
        self._build_product_profit_page(self.page_frames["product_profit"])
        self._build_orders_page(self.page_frames["orders"])
        self._build_product_daily_page(self.page_frames["product_daily"])
        self._build_product_series_page(self.page_frames["series"])
        self._build_cost_page(self.page_frames["costs"])
        self._build_cross_border_page(self.page_frames["cross_border"])
        self._build_listing_page(self.page_frames["listing"])
        self._build_weekly_page(self.page_frames["weekly"])
        self._build_inventory_page(self.page_frames["inventory"])
        self._build_supply_page(self.page_frames["supply"])
        self._build_sync_page(self.page_frames["sync"])
        self._build_settings_page(self.page_frames["settings"])
        self._build_feishu_page(self.page_frames["feishu"])
        self._build_shipment_tracking_page(self.page_frames["shipment_tracking"])

    def _page_header(self, parent: tk.Frame, title: str, subtitle: str, include_filters: bool = True) -> tk.Frame:
        header = tk.Frame(parent, bg=PALETTE.bg)
        header.pack(fill="x", padx=32, pady=(28, 18))
        title_block = tk.Frame(header, bg=PALETTE.bg)
        title_block.pack(side="left")
        tk.Label(title_block, text=title, font=("Microsoft YaHei UI", 24, "bold"),
                 fg=PALETTE.text, bg=PALETTE.bg).pack(anchor="w")
        tk.Label(title_block, text=subtitle, font=("Microsoft YaHei UI", 9),
                 fg=PALETTE.muted, bg=PALETTE.bg).pack(anchor="w", pady=(4, 0))
        if include_filters:
            filters = tk.Frame(
                header, bg=PALETTE.panel, highlightbackground=PALETTE.border,
                highlightthickness=1, padx=10, pady=8,
            )
            filters.pack(side="right", pady=2)
            preset = ttk.Combobox(
                filters, width=8, state="normal", textvariable=self.date_preset_var,
                values=("近7天", "近30天", "本月", "上月", "自定义"),
            )
            date_preset_values = ("近7天", "近30天", "本月", "上月", "自定义")
            preset.pack(side="left", padx=(0, 8))
            preset.bind("<<ComboboxSelected>>", self._apply_date_preset)
            preset.bind("<Return>", self._apply_date_preset)
            preset.bind(
                "<KeyRelease>",
                lambda event, widget=preset, values=date_preset_values: self._filter_combo_keyrelease(
                    event, widget, values, self.date_preset_var,
                ),
            )
            tk.Label(filters, text="日期", bg=PALETTE.panel, fg=PALETTE.muted,
                     font=("Microsoft YaHei UI", 9)).pack(side="left", padx=(0, 7))
            start_entry = ttk.Entry(filters, width=12, textvariable=self.date_from_var)
            start_entry.pack(side="left")
            tk.Label(filters, text="至", bg=PALETTE.panel, fg=PALETTE.muted,
                     font=("Microsoft YaHei UI", 9)).pack(side="left", padx=6)
            end_entry = ttk.Entry(filters, width=12, textvariable=self.date_to_var)
            end_entry.pack(side="left")
            for entry in (start_entry, end_entry):
                entry.bind("<Return>", lambda _event: self.refresh_all())
                entry.bind("<KeyRelease>", lambda _event: self.date_preset_var.set("自定义"))
            self._button(filters, "刷新", self.refresh_all, compact=True).pack(side="left", padx=(9, 0))
        return header

    def _build_dashboard_page(self, page: tk.Frame) -> None:
        self._page_header(page, "经营总览", "汇总 Seller 与 Performance API 数据")
        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26)
        self.metric_labels: dict[str, tk.Label] = {}
        definitions = [
            ("revenue", "总销售额", "₽", PALETTE.primary),
            ("orders", "下单件数", "件", PALETTE.green),
            ("spend", "广告花费", "₽", PALETTE.amber),
            ("roas", "广告 ROAS", "x", "#7A5AF8"),
        ]
        for index, (key, title, unit, color) in enumerate(definitions):
            card = self._metric_card(cards, title, unit, color)
            card.grid(row=0, column=index, sticky="nsew", padx=(0 if index == 0 else 6, 0 if index == 3 else 6))
            cards.grid_columnconfigure(index, weight=1)
            self.metric_labels[key] = card.value_label  # type: ignore[attr-defined]

        charts = tk.Frame(page, bg=PALETTE.bg)
        charts.pack(fill="both", expand=True, padx=26, pady=14)
        left = self._panel(charts)
        left.pack(side="left", fill="both", expand=True, padx=(0, 7))
        right = self._panel(charts, width=340)
        right.pack(side="right", fill="y", padx=(7, 0))
        right.pack_propagate(False)

        trend_head = tk.Frame(left, bg="#FFFFFF")
        trend_head.pack(fill="x", padx=18, pady=(15, 8))
        trend_text = tk.Frame(trend_head, bg="#FFFFFF")
        trend_text.pack(side="left")
        self.trend_title_label = tk.Label(
            trend_text, text="销售额与广告花费趋势",
            font=("Microsoft YaHei UI", 11, "bold"), fg=PALETTE.text, bg="#FFFFFF",
        )
        self.trend_title_label.pack(anchor="w")
        self.trend_subtitle_label = tk.Label(
            trend_text, text="按天汇总，单位：卢布",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg="#FFFFFF",
        )
        self.trend_subtitle_label.pack(anchor="w", pady=(2, 0))
        self.trend_mode_var = tk.StringVar(value="revenue")
        trend_switch = tk.Frame(trend_head, bg="#F1F5FA", padx=2, pady=2)
        trend_switch.pack(side="right")
        self.trend_mode_buttons: dict[str, tk.Button] = {}
        for mode, label in (("revenue", "销售额"), ("volume", "销量")):
            button = tk.Button(
                trend_switch, text=label, command=lambda selected=mode: self._set_trend_mode(selected),
                relief="flat", bd=0, cursor="hand2", font=("Microsoft YaHei UI", 8, "bold"),
                padx=12, pady=5,
            )
            button.pack(side="left")
            self.trend_mode_buttons[mode] = button
        self._set_trend_mode("revenue", redraw=False)
        self.trend_canvas = tk.Canvas(left, bg="#FFFFFF", highlightthickness=0, height=280)
        self.trend_canvas.pack(fill="both", expand=True, padx=16, pady=(0, 15))
        self.trend_canvas.bind("<Configure>", lambda _event: self._draw_trend())
        self.trend_canvas.bind("<Motion>", self._trend_hover)
        self.trend_canvas.bind("<Leave>", lambda _event: self.trend_canvas.delete("trend_hover"))

        self._panel_title(right, "经营健康度", "快速识别需要关注的指标")
        self.health_frame = tk.Frame(right, bg="#FFFFFF")
        self.health_frame.pack(fill="both", expand=True, padx=18, pady=(3, 16))
        self.health_labels: dict[str, tk.Label] = {}
        for key, label in [("acos", "广告 ACOS"), ("tacos", "整体 TACOS"), ("drr", "整体 DRR"),
                           ("ctr", "广告 CTR"), ("conversion", "浏览-下单转化"),
                           ("returns", "退货件数"), ("delivered", "妥投件数"),
                           ("cancellations", "取消件数"),
                           ("cancellation_rate", "产品取消率")]:
            row = tk.Frame(self.health_frame, bg="#FFFFFF")
            row.pack(fill="x", pady=6)
            tk.Label(row, text=label, font=("Microsoft YaHei UI", 9), fg=PALETTE.muted,
                     bg="#FFFFFF").pack(side="left")
            value = tk.Label(row, text="--", font=("Microsoft YaHei UI", 10, "bold"),
                             fg=PALETTE.text, bg="#FFFFFF")
            value.pack(side="right")
            self.health_labels[key] = value

    def _build_advertising_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "广告数据", "广告消耗、归因销售额与付费效率；所有汇总仅使用当前日期范围内已同步的活动日报"
        )
        summary = tk.Frame(page, bg=PALETTE.bg)
        summary.pack(fill="x", padx=26, pady=(0, 12))
        self.ad_summary_labels: dict[str, tk.Label] = {}
        summary_definitions = [
            ("spend", "广告消耗", "₽", PALETTE.amber),
            ("ad_revenue", "广告销售额", "₽", PALETTE.primary),
            ("ad_orders", "广告订单", "单", PALETTE.green),
            ("ctr", "平均 CTR", "%", "#7A5AF8"),
            ("cpc", "平均 CPC", "₽", "#F79009"),
            ("roas", "付费 ROAS", "x", "#12B76A"),
        ]
        for index, (key, title, unit, color) in enumerate(summary_definitions):
            card = self._metric_card(summary, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=4)
            summary.grid_columnconfigure(index, weight=1)
            self.ad_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        intelligence = tk.Frame(page, bg=PALETTE.bg, height=246)
        intelligence.pack(fill="x", padx=26, pady=(0, 12))
        intelligence.pack_propagate(False)
        scope_panel = self._panel(intelligence)
        scope_panel.pack(side="left", fill="both", expand=True, padx=(0, 6))
        funnel_panel = self._panel(intelligence, width=390)
        funnel_panel.pack(side="right", fill="y", padx=(6, 0))
        funnel_panel.pack_propagate(False)

        self._panel_title(
            scope_panel, "真实广告数据范围",
            "按当前店铺和日期范围汇总活动级日报；SKU 归因明细不会重复计入全店广告指标。",
        )
        self.ad_scope_label = tk.Label(
            scope_panel, text="等待广告数据加载", justify="left", anchor="nw", wraplength=600,
            font=("Microsoft YaHei UI", 9), fg=PALETTE.muted, bg="#FFFFFF",
        )
        self.ad_scope_label.pack(fill="both", expand=True, padx=18, pady=(2, 18))

        self._panel_title(funnel_panel, "广告转化漏斗", "从曝光到下单的可计算路径")
        self.ad_funnel_labels: dict[str, tk.Label] = {}
        funnel_body = tk.Frame(funnel_panel, bg="#FFFFFF")
        funnel_body.pack(fill="both", expand=True, padx=16, pady=(0, 12))
        steps = [
            ("impressions", "曝光", "#EEF4FF"),
            ("clicks", "点击", "#F4F3FF"),
            ("cart_adds", "加购", "#ECFDF3"),
            ("orders", "订单", "#FFF7ED"),
        ]
        for index, (key, title, color) in enumerate(steps):
            holder = tk.Frame(funnel_body, bg=color, height=34)
            holder.pack(fill="x", padx=12 * index, pady=(2, 5))
            holder.pack_propagate(False)
            tk.Label(
                holder, text=title, font=("Microsoft YaHei UI", 8, "bold"),
                fg=PALETTE.muted, bg=color,
            ).pack(side="left", padx=10)
            value = tk.Label(
                holder, text="—", font=("Segoe UI", 10, "bold"), fg=PALETTE.text, bg=color,
            )
            value.pack(side="right", padx=10)
            self.ad_funnel_labels[key] = value
        self.ad_funnel_note = tk.Label(
            funnel_panel, text="点击→加购 —  ·  点击→下单 —  ·  每单广告费 —",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg="#FFFFFF",
        )
        self.ad_funnel_note.pack(anchor="w", padx=18, pady=(0, 15))

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 20))
        filter_head = tk.Frame(panel, bg="#FFFFFF")
        filter_head.pack(fill="x", padx=16, pady=(13, 5))
        title_block = tk.Frame(filter_head, bg="#FFFFFF")
        title_block.pack(side="left")
        tk.Label(title_block, text="广告计划表现", font=("Microsoft YaHei UI", 11, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(anchor="w")
        tk.Label(title_block, text="活动级数据按花费排序；可结合状态、计费方式、最低花费与 ACOS 控制线筛选",
                 font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg="#FFFFFF").pack(anchor="w", pady=(2, 0))
        self.ad_filter_count_label = tk.Label(filter_head, text="0 条", font=("Microsoft YaHei UI", 9, "bold"),
                                              fg=PALETTE.primary, bg="#FFFFFF")
        self.ad_filter_count_label.pack(side="right", pady=5)

        self.ad_filter_vars = {
            "query": tk.StringVar(), "state": tk.StringVar(value="全部"),
            "payment_type": tk.StringVar(value="全部"), "min_spend": tk.StringVar(),
            "max_acos": tk.StringVar(),
        }
        filters = tk.Frame(panel, bg="#FFFFFF")
        filters.pack(fill="x", padx=16, pady=(2, 5))

        def filter_field(column: int, label: str, variable: tk.StringVar,
                         width: int, combo: bool = False) -> tk.Widget:
            block = tk.Frame(filters, bg="#FFFFFF")
            block.grid(row=0, column=column, sticky="ew", padx=(0, 7))
            tk.Label(block, text=label, font=("Microsoft YaHei UI", 8), fg=PALETTE.muted,
                     bg="#FFFFFF").pack(anchor="w", pady=(0, 3))
            if combo:
                widget: tk.Widget = ttk.Combobox(
                    block, textvariable=variable, values=("全部",), width=width, state="normal"
                )
            else:
                widget = ttk.Entry(block, textvariable=variable, width=width)
            widget.pack(fill="x")
            return widget

        query_entry = filter_field(0, "活动名称 / ID", self.ad_filter_vars["query"], 25)
        query_entry.bind("<Return>", lambda _event: self.refresh_advertising())
        query_entry.bind(
            "<KeyRelease>",
            lambda _event: self._debounced("ad_query", self.refresh_advertising),
        )
        self.ad_state_combo = filter_field(1, "状态", self.ad_filter_vars["state"], 13, combo=True)
        self.ad_payment_combo = filter_field(2, "计费方式", self.ad_filter_vars["payment_type"], 13, combo=True)
        self.ad_state_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.ad_state_combo, getattr(self, "_ad_state_values", ("全部",)),
                self.ad_filter_vars["state"],
            ),
        )
        self.ad_payment_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.ad_payment_combo,
                getattr(self, "_ad_payment_values", ("全部",)),
                self.ad_filter_vars["payment_type"],
            ),
        )
        self.ad_state_combo.bind("<<ComboboxSelected>>", lambda _event: self.refresh_advertising())
        self.ad_payment_combo.bind("<<ComboboxSelected>>", lambda _event: self.refresh_advertising())
        filter_field(3, "最低花费", self.ad_filter_vars["min_spend"], 11)
        filter_field(4, "最高 ACOS (%)", self.ad_filter_vars["max_acos"], 11)
        actions = tk.Frame(filters, bg="#FFFFFF")
        actions.grid(row=0, column=5, sticky="s", pady=(0, 1))
        self._button(actions, "应用", self.refresh_advertising, compact=True).pack(side="left")
        self._button(actions, "重置", self.reset_ad_filters, secondary=True, compact=True).pack(side="left", padx=(6, 0))
        filters.grid_columnconfigure(0, weight=2)
        for column in range(1, 5):
            filters.grid_columnconfigure(column, weight=1)
        columns = [
            ("campaign_name", "广告计划", 210, "w"), ("impressions", "曝光", 80, "e"),
            ("clicks", "点击", 70, "e"), ("ctr", "CTR", 68, "e"),
            ("cart_adds", "加购", 70, "e"), ("conversion", "点击-下单", 88, "e"),
            ("orders", "订单", 64, "e"), ("spend", "消耗", 88, "e"),
            ("cpc", "CPC", 76, "e"), ("cpa", "每单广告费", 100, "e"),
            ("revenue", "广告销售额", 105, "e"), ("roas", "付费 ROAS", 85, "e"),
            ("acos", "ACOS", 72, "e"),
        ]
        self.ad_tree = self._tree(panel, columns)
        self._refresh_campaign_filter_options()


    def _build_sales_page(self, page: tk.Frame) -> None:
        self._page_header(page, "商品销量", "按 SKU 与商家货号查看销售、履约和转化")
        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 20))
        toolbar = tk.Frame(panel, bg="#FFFFFF")
        toolbar.pack(fill="x", padx=16, pady=(14, 3))
        tk.Label(toolbar, text="商品表现", font=("Microsoft YaHei UI", 12, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        self.sales_search_var = tk.StringVar()
        search = ttk.Entry(toolbar, textvariable=self.sales_search_var, width=28)
        search.pack(side="right")
        search.bind(
            "<KeyRelease>", lambda _event: self._debounced("sales_query", self.refresh_sales)
        )
        tk.Label(toolbar, text="搜索 SKU/货号/商品名", font=("Microsoft YaHei UI", 8), fg=PALETTE.muted,
                 bg="#FFFFFF").pack(side="right", padx=8)
        self.sales_data_hint = tk.Label(
            panel, text="", anchor="w", font=("Microsoft YaHei UI", 8),
            fg=PALETTE.amber, bg="#FFFFFF",
        )
        self.sales_data_hint.pack(fill="x", padx=16, pady=(0, 4))
        columns = [
            ("offer_id", "货号", 115, "w"), ("sku", "Ozon SKU", 95, "w"),
            ("product_name", "商品名称", 220, "w"),
            ("orders", "下单", 65, "e"), ("delivered", "妥投", 65, "e"),
            ("returns", "退货", 60, "e"), ("cancellations", "取消", 60, "e"),
            ("cancellation_rate", "取消率", 75, "e"),
            ("views", "浏览", 70, "e"), ("cart_adds", "加购", 65, "e"),
            ("conversion", "转化率", 75, "e"),
            ("revenue", "销售额", 120, "e"),
        ]
        self.sales_tree = self._tree(panel, columns)

    def _build_ai_page(self, page: tk.Frame) -> None:
        self._page_header(page, "AI 分析", "将本地汇总数据发送给你配置的 OpenAI 兼容中转站")
        workspace = tk.Frame(page, bg=PALETTE.bg)
        workspace.pack(fill="both", expand=True, padx=26, pady=(0, 20))

        controls = self._panel(workspace, width=325)
        controls.pack(side="left", fill="y", padx=(0, 7))
        controls.pack_propagate(False)
        self._panel_title(controls, "选择分析方向", "快速分析或输入你自己的问题")
        tk.Label(
            controls, text="数据范围", font=("Microsoft YaHei UI", 9, "bold"),
            fg=PALETTE.text, bg="#FFFFFF",
        ).pack(anchor="w", padx=18, pady=(0, 5))
        self.ai_product_var = tk.StringVar(value="全部经营数据（商品/活动 Top 20）")
        self.ai_product_combo = ttk.Combobox(
            controls, state="normal", textvariable=self.ai_product_var, width=30,
        )
        self.ai_product_combo.pack(fill="x", padx=18, pady=(0, 10))
        self.ai_product_combo.bind("<KeyRelease>", self._filter_ai_products)
        self.ai_product_combo.bind(
            "<<ComboboxSelected>>", lambda _event: self._update_ai_product_ad_status()
        )
        self.ai_product_combo.bind(
            "<FocusOut>", lambda _event: self._update_ai_product_ad_status()
        )
        self._ai_products_by_display: dict[str, dict[str, Any]] = {}
        self.ai_product_ad_status_var = tk.StringVar(
            value="全店模式：只读取数据中心本地缓存，不会请求 Ozon"
        )
        tk.Label(
            controls, textvariable=self.ai_product_ad_status_var,
            justify="left", wraplength=280, font=("Microsoft YaHei UI", 8),
            fg=PALETTE.muted, bg="#FFFFFF",
        ).pack(anchor="w", padx=18, pady=(0, 10))
        preset = tk.Frame(controls, bg="#FFFFFF")
        preset.pack(fill="x", padx=18)
        for label, analysis_type in (
            ("经营诊断", "overview"), ("广告优化", "advertising"), ("商品销量", "products")
        ):
            self._button(
                preset, label, lambda kind=analysis_type: self.start_ai_analysis(kind), secondary=True
            ).pack(fill="x", pady=3)

        tk.Label(controls, text="自定义问题", font=("Microsoft YaHei UI", 9, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(anchor="w", padx=18, pady=(15, 5))
        question_holder = tk.Frame(controls, bg="#FFFFFF")
        question_holder.pack(fill="x", padx=18)
        self.ai_question_text = tk.Text(
            question_holder, height=7, wrap="word", relief="solid", bd=1,
            highlightthickness=0, font=("Microsoft YaHei UI", 9), fg=PALETTE.text,
            bg="#FFFFFF", padx=8, pady=7,
        )
        self.ai_question_text.pack(fill="x")
        self.ai_question_text.insert("1.0", "请结合当前日期范围的数据，找出最需优先优化的问题。")
        self._button(controls, "开始 AI 分析", lambda: self.start_ai_analysis("custom")).pack(
            fill="x", padx=18, pady=(12, 7)
        )
        tk.Label(
            controls,
            text=(
                "全店模式发送 Top 20，控制请求体大小；\n"
                "选择商品后发送：全店整体广告效率 +\n"
                "该 SKU 的本地广告、经营明细和库存；\n"
                "广告只读数据中心缓存，AI 页面不请求 Ozon。"
            ),
            justify="left", wraplength=280, font=("Microsoft YaHei UI", 8),
            fg=PALETTE.muted, bg="#FFFFFF",
        ).pack(anchor="w", padx=18, pady=(3, 15))
        self.refresh_ai_product_options()

        result_panel = self._panel(workspace)
        result_panel.pack(side="right", fill="both", expand=True, padx=(7, 0))
        result_head = tk.Frame(result_panel, bg="#FFFFFF")
        result_head.pack(fill="x", padx=18, pady=(15, 8))
        title_block = tk.Frame(result_head, bg="#FFFFFF")
        title_block.pack(side="left")
        tk.Label(title_block, text="AI 分析结果", font=("Microsoft YaHei UI", 11, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(anchor="w")
        self.ai_result_hint = tk.Label(title_block, text="等待分析", font=("Microsoft YaHei UI", 8),
                                       fg=PALETTE.muted, bg="#FFFFFF")
        self.ai_result_hint.pack(anchor="w", pady=(2, 0))
        self._button(result_head, "清空", self.clear_ai_result, secondary=True, compact=True).pack(side="right")
        self._button(result_head, "导出 PDF", self.export_ai_as_pdf, secondary=True, compact=True).pack(
            side="right", padx=(0, 6)
        )
        self._button(result_head, "导出 Word", self.export_ai_as_word, secondary=True, compact=True).pack(
            side="right", padx=(0, 6)
        )
        result_holder = tk.Frame(result_panel, bg="#FFFFFF")
        result_holder.pack(fill="both", expand=True, padx=18, pady=(0, 16))
        self.ai_output_text = tk.Text(
            result_holder, wrap="word", relief="flat", bd=0, bg="#F8FAFD",
            fg="#344054", font=("Microsoft YaHei UI", 10), padx=14, pady=12, state="disabled",
        )
        result_scrollbar = ttk.Scrollbar(result_holder, orient="vertical", command=self.ai_output_text.yview)
        self.ai_output_text.configure(yscrollcommand=result_scrollbar.set)
        result_scrollbar.pack(side="right", fill="y")
        self.ai_output_text.pack(side="left", fill="both", expand=True)
        self.ai_output_text.tag_configure(
            "h1", font=("Microsoft YaHei UI", 15, "bold"), foreground=PALETTE.text,
            spacing1=9, spacing3=6,
        )
        self.ai_output_text.tag_configure(
            "h2", font=("Microsoft YaHei UI", 12, "bold"), foreground=PALETTE.primary,
            spacing1=8, spacing3=4,
        )
        self.ai_output_text.tag_configure(
            "h3", font=("Microsoft YaHei UI", 10, "bold"), foreground=PALETTE.text,
            spacing1=6, spacing3=3,
        )
        self.ai_output_text.tag_configure("bold", font=("Microsoft YaHei UI", 10, "bold"))

    def _build_profit_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "月度盈亏表", "按月汇总销售、平台净回款、费用与本地产品成本",
            include_filters=False,
        )
        month_bar = self._panel(page)
        month_bar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(month_bar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.profit_month_var = tk.StringVar(value=date.today().strftime("%Y-%m"))
        tk.Label(controls, text="核算月份", font=("Microsoft YaHei UI", 9, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        ttk.Entry(controls, width=9, textvariable=self.profit_month_var).pack(side="left", padx=(8, 5))
        self._button(controls, "上月", lambda: self._change_profit_month(-1), secondary=True,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "下月", lambda: self._change_profit_month(1), secondary=True,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "刷新本地报表", self.refresh_profit, secondary=True,
                     compact=True).pack(side="left", padx=(7, 3))
        self._button(controls, "同步本月销量", self.sync_profit_sales,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "同步应计费用", self.sync_profit_accruals,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "店铺费用与对账", self.open_profit_store_detail,
                     secondary=True, compact=True).pack(side="left", padx=3)
        tk.Label(
            controls, text="销量按订单日期；应计费用按财务发生日期",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg="#FFFFFF",
        ).pack(side="right")
        tax_controls = tk.Frame(month_bar, bg="#FFFFFF")
        tax_controls.pack(fill="x", padx=16, pady=(0, 10))
        self.profit_tax_rate_var = tk.StringVar(
            value=self.database.get_setting("local_tax_rate", "6")
        )
        self.profit_payout_fee_rate_var = tk.StringVar(
            value=self.database.get_setting("local_payout_fee_rate", "1")
        )
        self.profit_delivery_default_var = tk.StringVar(
            value=self.database.get_setting("local_default_delivery_fee", "0")
        )
        tk.Label(
            tax_controls, text="俄罗斯税费估算", bg="#FFFFFF", fg=PALETTE.text,
            font=("Microsoft YaHei UI", 8, "bold"),
        ).pack(side="left")
        tk.Label(tax_controls, text="税率 %", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left", padx=(12, 4))
        ttk.Entry(tax_controls, width=7, textvariable=self.profit_tax_rate_var).pack(side="left")
        tk.Label(tax_controls, text="回款手续费 %", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left", padx=(12, 4))
        ttk.Entry(tax_controls, width=7, textvariable=self.profit_payout_fee_rate_var).pack(side="left")
        tk.Label(tax_controls, text="无历史配送时/件 ₽", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left", padx=(12, 4))
        ttk.Entry(tax_controls, width=8, textvariable=self.profit_delivery_default_var).pack(side="left")
        self._button(
            tax_controls, "保存参数", self.save_profit_parameters,
            secondary=True, compact=True,
        ).pack(side="left", padx=8)
        tk.Label(
            tax_controls,
            text=("默认按 USN 收入制 6% 估算；2026 年起还需按主体年收入"
                  "判断 VAT 免税或 5%/7%/22% 口径，请以会计确认后修改。"),
            bg="#FFFFFF", fg=PALETTE.amber, font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.profit_summary_labels: dict[str, tk.Label] = {}
        self.profit_metric_cards: dict[str, tk.Frame] = {}
        self.profit_metric_card_colors: dict[str, str] = {}
        for index, (key, title, unit, color) in enumerate([
            ("revenue", "订单金额", "₽", PALETTE.primary),
            ("orders", "已订购商品", "件", "#7A5AF8"),
            ("sales_returns", "销售/退货应计", "₽", "#2563EB"),
            ("accrual_fees", "应计费用", "₽", PALETTE.amber),
            ("other_adjustments", "其他收入/调整", "₽", "#14B8A6"),
            ("platform_payout", "应计净额", "₽", PALETTE.green),
            ("product_cost", "成本核算金额", "₽", "#EF4444"),
            ("profit", "税前利润", "₽", "#0EA5E9"),
            ("tax_amount", "税费估算", "₽", "#F04438"),
            ("payout_fee", "回款手续费", "₽", "#F79009"),
            ("after_tax_profit", "税后利润", "₽", "#12B76A"),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            row_index, column_index = divmod(index, 6)
            card.grid(row=row_index, column=column_index, sticky="nsew", padx=5, pady=3)
            cards.grid_columnconfigure(column_index, weight=1)
            self.profit_summary_labels[key] = card.value_label  # type: ignore[attr-defined]
            self.profit_metric_cards[key] = card
            self.profit_metric_card_colors[key] = color
            self._bind_click_tree(
                card, lambda _event, metric=key: self.filter_profit_metric(metric)
            )

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 10))
        editor = tk.Frame(panel, bg="#FFFFFF")
        editor.pack(fill="x", padx=16, pady=(12, 2))
        tk.Label(editor, text="SKU 销量与应计费用明细", font=("Microsoft YaHei UI", 11, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        tk.Label(
            editor, text="双击 SKU 查看全部 API 字段、费用分类，并填写单件成本与头程",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.primary, bg="#FFFFFF",
        ).pack(side="right")

        product_search = tk.Frame(panel, bg="#FFFFFF")
        product_search.pack(fill="x", padx=16, pady=(5, 1))
        self.profit_product_var = tk.StringVar()
        self._profit_products_by_display: dict[str, dict[str, Any]] = {}
        self._profit_product_values: tuple[str, ...] = ()
        tk.Label(
            product_search, text="查找成本商品", font=("Microsoft YaHei UI", 8, "bold"),
            fg=PALETTE.text, bg="#FFFFFF",
        ).pack(side="left")
        self.profit_product_combo = ttk.Combobox(
            product_search, state="normal", width=38,
            textvariable=self.profit_product_var,
        )
        self.profit_product_combo.pack(side="left", padx=(8, 6))
        self.profit_product_combo.bind("<KeyRelease>", self._filter_profit_products)
        self.profit_product_combo.bind(
            "<<ComboboxSelected>>", lambda _event: self.open_profit_product_from_combo()
        )
        self.profit_product_combo.bind(
            "<Return>", lambda _event: self.open_profit_product_from_combo()
        )
        self._button(
            product_search, "填写成本", self.open_profit_product_from_combo,
            compact=True,
        ).pack(side="left")
        tk.Label(
            product_search,
            text="可输入货号或 SKU；例如输入 GJYB 会展开全部匹配商品",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg="#FFFFFF",
        ).pack(side="left", padx=10)

        self.profit_hint = tk.Label(
            panel, text="", anchor="w", font=("Microsoft YaHei UI", 8),
            fg=PALETTE.amber, bg="#FFFFFF",
        )
        self.profit_hint.pack(fill="x", padx=16, pady=(3, 0))
        filter_bar = tk.Frame(panel, bg="#FFFFFF")
        filter_bar.pack(fill="x", padx=16, pady=(3, 0))
        self.profit_metric_filter_var = tk.StringVar(
            value="点击上方指标卡，可在下方查看对应的 SKU 或 API 费用字段"
        )
        tk.Label(
            filter_bar, textvariable=self.profit_metric_filter_var, anchor="w",
            font=("Microsoft YaHei UI", 8, "bold"), fg=PALETTE.primary, bg="#FFFFFF",
        ).pack(side="left")
        self._button(
            filter_bar, "清除指标筛选", self.clear_profit_metric_filter,
            secondary=True, compact=True,
        ).pack(side="right")
        self.profit_notebook = ttk.Notebook(panel)
        self.profit_notebook.pack(fill="both", expand=True, padx=10, pady=(5, 10))
        sku_tab = tk.Frame(self.profit_notebook, bg="#FFFFFF")
        unallocated_tab = tk.Frame(self.profit_notebook, bg="#FFFFFF")
        metric_tab = tk.Frame(self.profit_notebook, bg="#FFFFFF")
        self.profit_notebook.add(sku_tab, text="SKU 应计费用")
        self.profit_notebook.add(unallocated_tab, text="店铺级 / 无法分摊费用")
        self.profit_notebook.add(metric_tab, text="指标钻取")
        self.profit_notebook.bind("<<NotebookTabChanged>>", self._profit_tab_changed)
        self.profit_tree = self._tree(sku_tab, [
            ("offer_id", "货号", 115, "w"), ("sku", "Ozon SKU", 95, "w"),
            ("orders", "已订购商品", 85, "e"), ("revenue", "订单金额", 105, "e"),
            ("cost_units", "成本核算件数", 95, "e"),
            ("product_cost_total", "成本核算金额", 105, "e"),
            ("first_mile_total", "头程核算金额", 105, "e"),
            ("sales_returns", "销售与退货（应计）", 125, "e"),
            ("payout", "应计净额", 100, "e"), ("commission", "Ozon 佣金", 90, "e"),
            ("ad_spend", "商品广告费", 95, "e"), ("delivery", "配送物流", 90, "e"),
            ("last_mile", "后段配送", 90, "e"), ("avg_delivery", "平均配送费", 95, "e"),
            ("returns", "退货金额", 90, "e"), ("return_logistics", "退货物流", 90, "e"),
            ("acquiring", "收单费", 80, "e"), ("fbo_storage", "FBO/仓储/包装", 115, "e"),
            ("adjustments", "赔偿/罚款/调整", 115, "e"), ("other_fees", "其他应计", 90, "e"),
            ("unit_cost", "单件成本 RUB", 100, "e"),
            ("first_mile", "单件头程 RUB", 100, "e"), ("profit", "暂估利润", 100, "e"),
            ("profit_rate", "利润率", 75, "e"),
        ])
        self.profit_tree.bind("<Double-1>", self.open_profit_sku_detail)
        self.profit_tree.bind("<Return>", self.open_profit_sku_detail)
        self._profit_rows_by_sku: dict[str, dict[str, Any]] = {}
        self._profit_rows: list[dict[str, Any]] = []
        self._profit_summary: dict[str, Any] = {}
        self._profit_active_metric = ""
        self.profit_unallocated_hint = tk.Label(
            unallocated_tab,
            text="逐项列出没有唯一 SKU 的越库、按订单推广、仓储、订阅、赔偿及其他店铺级应计费用；不会重复计入 SKU。",
            anchor="w", font=("Microsoft YaHei UI", 8), fg=PALETTE.amber, bg="#FFFFFF",
        )
        self.profit_unallocated_hint.pack(fill="x", padx=16, pady=(10, 0))
        self.profit_unallocated_tree = self._tree(unallocated_tab, [
            ("date", "发生时间", 135, "w"), ("operation_id", "应计费用 ID", 145, "w"),
            ("category", "费用大类", 125, "w"), ("name", "费用项目（中文）", 190, "w"),
            ("api_name", "API 原始名称", 285, "w"), ("amount", "金额", 105, "e"),
            ("reason", "无法分摊原因", 185, "w"), ("skus", "涉及 SKU", 170, "w"),
            ("posting", "订单/包裹号", 145, "w"), ("operation", "业务名称", 190, "w"),
        ])
        self.profit_metric_tree = self._tree(metric_tab, [
            ("scope", "对应指标", 135, "w"), ("category", "费用大类", 155, "w"),
            ("name", "费用项目（中文）", 230, "w"),
            ("api_name", "API 原始字段/名称", 390, "w"),
            ("rows", "记录数", 90, "e"), ("amount", "金额", 135, "e"),
        ])
        self._profit_unallocated_loaded = False
        self._profit_unallocated_date_range = ("", "")

    def _build_product_profit_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "产品利润表",
            "单件预测：每卖 1 件商品的售价、广告、平台费用、成本及税前/税后利润",
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.product_profit_search_var = tk.StringVar()
        tk.Label(
            controls, text="模糊搜索货号 / Ozon SKU", bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left")
        search = ttk.Entry(controls, width=30, textvariable=self.product_profit_search_var)
        search.pack(side="left", padx=(7, 10))
        search.bind(
            "<KeyRelease>",
            lambda _event: self._debounced("product_profit_query", self.refresh_product_profit),
        )
        self._button(
            controls, "刷新本地估算", self.refresh_product_profit,
            secondary=True, compact=True,
        ).pack(side="left")
        self._button(
            controls, "同步所选商品广告明细", self.sync_product_profit_ads,
            compact=True,
        ).pack(side="left", padx=7)
        tk.Label(
            controls,
            text="广告只读取精确 SKU 本地缓存；不会把通用活动强行分摊给商品",
            bg="#FFFFFF", fg=PALETTE.amber, font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.product_profit_summary_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("products", "参考 SKU", "个", "#7A5AF8"),
            ("revenue", "平均单件售价", "₽", PALETTE.primary),
            ("ad_spend", "平均单件广告", "₽", PALETTE.amber),
            ("pre_tax", "平均单件税前利润", "₽", "#0EA5E9"),
            ("after_tax", "平均单件税后利润", "₽", PALETTE.green),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=5)
            cards.grid_columnconfigure(index, weight=1)
            self.product_profit_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 12))
        self.product_profit_hint = tk.Label(
            panel, text="", anchor="w", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        )
        self.product_profit_hint.pack(fill="x", padx=16, pady=(11, 2))
        self.product_profit_tree = self._tree(panel, [
            ("offer_id", "货号", 125, "w"), ("sku", "Ozon SKU", 95, "w"),
            ("orders", "参考销量", 75, "e"), ("revenue", "单件售价", 90, "e"),
            ("ad_spend", "单件广告", 85, "e"),
            ("tacos", "TACOS", 70, "e"),
            ("delivery_unit", "基础配送/件", 95, "e"),
            ("delivery_source", "配送费来源", 105, "w"),
            ("commission", "单件佣金", 85, "e"), ("acquiring", "单件收单费", 90, "e"),
            ("product_cost", "单件采购成本", 100, "e"),
            ("first_mile", "单件头程 RUB", 105, "e"),
            ("tax", "单件税费", 80, "e"), ("payout_fee", "单件回款手续费", 105, "e"),
            ("pre_tax", "单件税前利润", 105, "e"),
            ("after_tax", "单件税后利润", 105, "e"),
        ])
        self.product_profit_tree.bind("<Double-1>", self.open_product_profit_detail)
        self.product_profit_tree.bind("<Return>", self.open_product_profit_detail)
        self._product_profit_rows_by_iid: dict[str, dict[str, Any]] = {}

    def _build_orders_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "订单中心",
            "按订单状态、履约方式和商品检索本地 Seller 缓存；金额随店铺类型显示为 RUB 或 CNY",
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.order_search_var = tk.StringVar()
        self.order_scheme_var = tk.StringVar(value="全部履约")
        tk.Label(controls, text="搜索订单号 / 货号 / SKU", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        search = ttk.Entry(controls, width=30, textvariable=self.order_search_var)
        search.pack(side="left", padx=(7, 10))
        search.bind("<KeyRelease>", lambda _event: self._debounced("orders", self.refresh_orders))
        scheme = ttk.Combobox(
            controls, width=13, state="readonly", textvariable=self.order_scheme_var,
            values=("全部履约", "FBO", "FBS", "FBP", "WHD"),
        )
        scheme.pack(side="left", padx=(0, 8))
        scheme.bind("<<ComboboxSelected>>", lambda _event: self.refresh_orders())
        self._button(controls, "刷新本地订单", self.refresh_orders, secondary=True, compact=True).pack(side="left")
        self._button(controls, "导入配送费表", self.import_delivery_tariff_workbook, compact=True).pack(side="left", padx=7)
        self.order_tariff_hint = tk.Label(
            controls, text="", bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        )
        self.order_tariff_hint.pack(side="right")

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        self.order_detail_tree = self._tree(panel, [
            ("day", "下单日期", 95, "w"), ("posting", "订单/货件编号", 155, "w"),
            ("status", "状态", 95, "w"), ("scheme", "履约", 70, "w"),
            ("offer", "货号", 150, "w"), ("sku", "Ozon SKU", 105, "w"),
            ("quantity", "数量", 60, "e"), ("price", "订单金额", 95, "e"),
            ("origin", "运输集群", 160, "w"), ("destination", "配送集群", 160, "w"),
            ("volume", "体积 L", 75, "e"), ("weight", "重量 kg", 80, "e"),
            ("fee_unit", "基础配送/件", 100, "e"),
            ("fee_total", "基础配送合计", 105, "e"), ("match", "匹配状态", 120, "w"),
        ])

    def _build_product_daily_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "每日产品经营",
            "按天、按 SKU 查看销量、销售额、精确商品广告、TACOS、每笔订单广告费及预估利润",
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.product_daily_search_var = tk.StringVar()
        tk.Label(controls, text="模糊搜索货号 / SKU", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        search = ttk.Entry(controls, width=32, textvariable=self.product_daily_search_var)
        search.pack(side="left", padx=(7, 10))
        search.bind(
            "<KeyRelease>",
            lambda _event: self._debounced("product_daily", self.refresh_product_daily),
        )
        self._button(controls, "刷新本地数据", self.refresh_product_daily, secondary=True, compact=True).pack(side="left")
        tk.Label(
            controls, text="优先官方 SKU 明细；历史仅识别名称唯一等于货号/SKU 的专属活动",
            bg="#FFFFFF", fg=PALETTE.amber, font=("Microsoft YaHei UI", 8),
        ).pack(side="right")
        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        self.product_daily_hint = tk.Label(
            panel, text="", bg="#FFFFFF", fg=PALETTE.muted, anchor="w",
            font=("Microsoft YaHei UI", 8),
        )
        self.product_daily_hint.pack(fill="x", padx=16, pady=(10, 2))
        self.product_daily_tree = self._tree(panel, [
            ("day", "日期", 95, "w"), ("offer", "货号", 155, "w"),
            ("sku", "Ozon SKU", 105, "w"), ("units", "销量", 65, "e"),
            ("revenue", "销售额", 100, "e"), ("ad_spend", "广告费", 90, "e"),
            ("ad_orders", "广告订单", 75, "e"),
            ("ad_source", "广告口径", 85, "w"),
            ("ad_per_order", "每笔订单广告费", 110, "e"),
            ("tacos", "TACOS", 70, "e"), ("profit", "当天预估利润", 105, "e"),
            ("profit_rate", "利润率", 75, "e"), ("status", "成本口径", 115, "w"),
        ])

    def _build_product_series_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "产品系列分析",
            "按货号系列汇总每天、每 7 天或每月的销量与销售额；只在你点击时发送飞书群",
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.product_series_search_var = tk.StringVar()
        tk.Label(controls, text="模糊搜索系列 / 货号 / SKU", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        search = ttk.Entry(controls, width=30, textvariable=self.product_series_search_var)
        search.pack(side="left", padx=(7, 10))
        search.bind("<KeyRelease>", lambda _event: self._debounced("series", self.refresh_product_series))
        self.product_series_period_var = tk.StringVar(value="每7天")
        period = ttk.Combobox(
            controls, width=9, state="readonly", textvariable=self.product_series_period_var,
            values=("每天", "每7天", "每月"),
        )
        period.pack(side="left", padx=(0, 8))
        period.bind("<<ComboboxSelected>>", lambda _event: self.refresh_product_series())
        self._button(controls, "刷新本地统计", self.refresh_product_series, secondary=True, compact=True).pack(side="left")
        self._button(
            controls, "发送到飞书群",
            self.send_product_series_to_feishu_group, compact=True,
        ).pack(side="right")
        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        self.product_series_hint = tk.Label(
            panel, text="", bg="#FFFFFF", fg=PALETTE.muted, anchor="w",
            font=("Microsoft YaHei UI", 8),
        )
        self.product_series_hint.pack(fill="x", padx=16, pady=(10, 2))
        self.product_series_tree = self._tree(panel, [
            ("period", "周期", 180, "w"), ("series", "产品系列", 180, "w"),
            ("sku_count", "SKU 数量", 90, "e"), ("units", "销量", 100, "e"),
            ("revenue", "销售额 RUB", 140, "e"),
        ])
        self._product_series_rows: list[dict[str, Any]] = []

    def _build_shipment_tracking_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "发货跟踪",
            "从飞书多维表格缓存采购发货记录；每日仅检测，到库通知由你双击记录后确认发送",
            include_filters=False,
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.shipment_tracking_search_var = tk.StringVar()
        tk.Label(
            controls, text="搜索采购单 / 批次 / 品名 / 店铺",
            bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left")
        search = ttk.Entry(
            controls, width=32, textvariable=self.shipment_tracking_search_var,
        )
        search.pack(side="left", padx=(7, 10))
        search.bind(
            "<KeyRelease>",
            lambda _event: self._debounced("shipment_tracking", self.refresh_shipment_tracking),
        )
        self._button(
            controls, "从飞书读取并检测到库", self.start_shipment_tracking_sync,
            compact=True,
        ).pack(side="left")
        self._button(
            controls, "刷新本地缓存", self.refresh_shipment_tracking,
            secondary=True, compact=True,
        ).pack(side="left", padx=7)
        self.shipment_tracking_auto_var = tk.BooleanVar(
            value=self.database.get_setting("shipment_tracking_auto_enabled", "1") != "0"
        )
        tk.Checkbutton(
            controls, text="软件运行时每天自动检查（不自动发送）", variable=self.shipment_tracking_auto_var,
            command=self._toggle_shipment_tracking_auto,
            bg="#FFFFFF", fg=PALETTE.text, activebackground="#FFFFFF",
            selectcolor="#FFFFFF", font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.shipment_tracking_summary_labels: dict[str, tk.Label] = {}
        for index, (key, title, color) in enumerate([
            ("total", "跟踪记录", PALETTE.primary),
            ("in_transit", "运输途中", PALETTE.amber),
            ("arrived", "国外到库", PALETTE.green),
            ("pending", "待发送通知", "#7A5AF8"),
        ]):
            card = self._metric_card(cards, title, "条", color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=5)
            cards.grid_columnconfigure(index, weight=1)
            self.shipment_tracking_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        self.shipment_tracking_hint = tk.Label(
            panel, text="", bg="#FFFFFF", fg=PALETTE.muted, anchor="w",
            font=("Microsoft YaHei UI", 8),
        )
        self.shipment_tracking_hint.pack(fill="x", padx=16, pady=(10, 2))
        self.shipment_tracking_tree = self._tree(panel, [
            ("tracking", "采购/跟踪单号", 150, "w"),
            ("product", "品名", 150, "w"), ("batch", "批次号", 145, "w"),
            ("shop", "店铺", 110, "w"), ("quantity", "数量", 65, "e"),
            ("status", "货物状态", 95, "w"), ("channel", "渠道", 90, "w"),
            ("domestic", "国内到库", 95, "w"), ("foreign", "国外到库", 95, "w"),
            ("notification", "飞书通知", 90, "w"), ("updated", "本地更新时间", 140, "w"),
        ])
        self.shipment_tracking_tree.bind(
            "<Double-1>", self.send_selected_shipment_to_feishu,
        )
        self.shipment_tracking_tree.bind(
            "<Return>", self.send_selected_shipment_to_feishu,
        )

    def _build_cost_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "产品成本", "采购成本按人民币填写，头程直接按卢布填写，并保存尺寸与重量",
            include_filters=False,
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        self.cost_search_var = tk.StringVar()
        self.exchange_rate_var = tk.StringVar(value=f"{self.database.rub_per_cny():.4f}")
        tk.Label(controls, text="模糊搜索货号 / SKU", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        search = ttk.Entry(controls, width=30, textvariable=self.cost_search_var)
        search.pack(side="left", padx=(6, 16))
        search.bind(
            "<KeyRelease>", lambda _event: self._debounced("cost_query", self.refresh_costs)
        )
        tk.Label(controls, text="1 人民币 =", bg="#FFFFFF", fg=PALETTE.text,
                 font=("Microsoft YaHei UI", 9, "bold")).pack(side="left")
        ttk.Entry(controls, width=9, textvariable=self.exchange_rate_var).pack(side="left", padx=(6, 4))
        tk.Label(controls, text="卢布", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        self._button(controls, "保存汇率并重算", self.save_exchange_rate,
                     compact=True).pack(side="left", padx=8)
        self._button(controls, "刷新", self.refresh_costs, secondary=True,
                     compact=True).pack(side="left")

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.cost_summary_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("products", "SKU 数量", "个", PALETTE.primary),
            ("configured", "已填采购成本", "个", PALETTE.green),
            ("missing", "待填写", "个", PALETTE.amber),
            ("rate", "当前人民币汇率", "RUB/CNY", "#7A5AF8"),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=5)
            cards.grid_columnconfigure(index, weight=1)
            self.cost_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        header = tk.Frame(panel, bg="#FFFFFF")
        header.pack(fill="x", padx=16, pady=(12, 4))
        tk.Label(header, text="店铺产品成本", font=("Microsoft YaHei UI", 11, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        tk.Label(
            header, text="双击 SKU 填写；支持按货号前缀批量统一成本与头程",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.primary, bg="#FFFFFF",
        ).pack(side="right")
        self.cost_tree = self._tree(panel, [
            ("offer", "货号", 180, "w"), ("sku", "Ozon SKU", 125, "w"),
            ("unit_cny", "采购成本 CNY", 115, "e"),
            ("first_rub", "头程 RUB", 105, "e"),
            ("rate", "RUB/CNY", 85, "e"),
            ("unit_rub", "采购成本 RUB", 115, "e"),
            ("dimensions", "长×宽×高 cm", 145, "e"),
            ("weight", "重量 kg", 85, "e"), ("note", "备注", 210, "w"),
        ])
        self.cost_tree.bind("<Double-1>", self.open_cost_editor)
        self.cost_tree.bind("<Return>", self.open_cost_editor)
        self._cost_rows_by_iid: dict[str, dict[str, Any]] = {}

    def _build_cross_border_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "跨境店利润", "仅显示当前日期内有出单的产品；全店广告费不分摊到单品",
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=9)
        self.cross_border_search_var = tk.StringVar()
        tk.Label(controls, text="搜索已出单货号 / SKU", bg="#FFFFFF",
                 fg=PALETTE.muted).pack(side="left")
        search = ttk.Entry(controls, width=34, textvariable=self.cross_border_search_var)
        search.pack(side="left", padx=(7, 10))
        search.bind(
            "<KeyRelease>",
            lambda _event: self._debounced(
                "cross_border_query", lambda: self.refresh_cross_border(reload_data=False)
            ),
        )
        self.cross_border_scheme_var = tk.StringVar(value="全部履约")
        scheme_combo = ttk.Combobox(
            controls, width=13, state="readonly",
            textvariable=self.cross_border_scheme_var,
            values=("全部履约", "FBP", "FBS / realFBS", "WHD", "其它 / 未识别"),
        )
        scheme_combo.pack(side="left", padx=(0, 8))
        scheme_combo.bind(
            "<<ComboboxSelected>>",
            lambda _event: self.refresh_cross_border(reload_data=False),
        )
        self._button(controls, "刷新报表", self.refresh_cross_border,
                     compact=True).pack(side="left")
        self.cross_border_sync_button = self._button(
            controls, "同步跨境店 API", lambda: self.start_sync("all"), compact=True,
        )
        self.cross_border_sync_button.pack(side="left", padx=5)
        self.cross_border_ledger_button = self._button(
            controls, "连接上品台账", self.configure_listing_ledger,
            secondary=True, compact=True,
        )
        self.cross_border_ledger_button.pack(side="left", padx=(0, 5))
        self.cross_border_ledger_sync_button = self._button(
            controls, "同步台账", self.sync_listing_ledger_costs,
            secondary=True, compact=True,
        )
        self.cross_border_ledger_sync_button.pack(side="left", padx=(0, 5))
        self.cross_border_listing_button = self._button(
            controls, "查看上品台账", lambda: self.show_page("listing"),
            secondary=True, compact=True,
        )
        self.cross_border_listing_button.pack(side="left", padx=(0, 5))
        self.cross_border_tool_button = self._button(
            controls, "打开上品工具", self.open_listing_tool,
            secondary=True, compact=True,
        )
        self.cross_border_tool_button.pack(side="left")
        self.cross_border_hint = tk.Label(
            controls, text="", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        )
        self.cross_border_hint.pack(side="right")

        rate_controls = tk.Frame(toolbar, bg="#FFFFFF")
        rate_controls.pack(fill="x", padx=16, pady=(0, 9))
        tk.Label(
            rate_controls, text="跨境利润换算汇率", bg="#FFFFFF",
            fg=PALETTE.text, font=("Microsoft YaHei UI", 9, "bold"),
        ).pack(side="left")
        tk.Label(rate_controls, text="1 人民币 =", bg="#FFFFFF",
                 fg=PALETTE.muted).pack(side="left", padx=(14, 4))
        ttk.Entry(rate_controls, width=9, textvariable=self.exchange_rate_var).pack(side="left")
        tk.Label(rate_controls, text="卢布", bg="#FFFFFF",
                 fg=PALETTE.muted).pack(side="left", padx=(4, 8))
        self._button(
            rate_controls, "应用汇率并重算", self.save_exchange_rate,
            compact=True,
        ).pack(side="left")
        tk.Label(
            rate_controls,
            text="实时利润按下单销售额计算；平台费只估算佣金/收单，全店广告另扣；运费计入产品成本。",
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(side="left", padx=(14, 0))

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.cross_border_summary_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("revenue", "销售额", "¥", PALETTE.primary),
            ("orders", "销量", "件", "#7A5AF8"),
            ("ad_spend", "全店广告费", "¥", PALETTE.amber),
            ("other_expenses", "预估佣金+收单", "¥", "#F04438"),
            ("cost", "采购+预估运费", "¥", "#0BA5EC"),
            ("profit", "实时预估利润", "¥", PALETTE.green),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=4)
            cards.grid_columnconfigure(index, weight=1)
            self.cross_border_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        product_panel = self._panel(page)
        product_panel.configure(height=345)
        product_panel.pack(fill="both", expand=True, padx=26, pady=(0, 10))
        self._panel_title(
            product_panel, "有出单产品利润明细",
            "实时商品贡献 = 下单销售额 - 历史费率预估佣金/收单 - 采购 - 预估运费；全店广告不分摊到单品。",
        )
        self.cross_border_tree = self._tree(product_panel, [
            ("offer", "货号", 155, "w"), ("sku", "Ozon SKU", 115, "w"),
            ("orders", "销量", 70, "e"), ("fulfillment", "履约单", 70, "e"),
            ("fbp", "FBP", 55, "e"), ("rfbs", "FBS", 55, "e"),
            ("whd", "WHD", 55, "e"), ("revenue", "销售额 CNY", 105, "e"),
            ("price_cny", "单件售价 CNY", 105, "e"),
            ("purchase_cny", "采购 CNY", 90, "e"), ("weight", "重量 kg", 75, "e"),
            ("freight_cny", "定价参考运费 CNY", 125, "e"),
            ("purchase_total", "采购合计 CNY", 115, "e"),
            ("freight_total", "运费合计 CNY", 110, "e"),
            ("estimated_fee", "预估佣金+收单 CNY", 135, "e"),
            ("contribution", "实时商品贡献 CNY", 135, "e"),
            ("finance_net", "Finance 已结算净额", 125, "e"),
            ("formula", "实时公式（销售-平台费-采购-运费）", 310, "w"),
        ])
        self.cross_border_tree.bind("<Double-1>", self.open_cross_border_cost_editor)
        self.cross_border_tree.bind("<Return>", self.open_cross_border_cost_editor)
        self._cross_border_rows_by_iid: dict[str, dict[str, Any]] = {}

        weekly_panel = self._panel(page)
        weekly_panel.configure(height=235)
        weekly_panel.pack(fill="x", padx=26, pady=(0, 12))
        weekly_panel.pack_propagate(False)
        self._panel_title(
            weekly_panel, "跨境周度总利润（CNY）",
            "实时利润 = 下单销售额 - 全店广告 - 预估佣金/收单 - 采购 - 预估运费；Finance 仅作已结算对账。",
        )
        self.cross_border_week_tree = self._tree(weekly_panel, [
            ("period", "周期", 170, "w"), ("orders", "销量", 75, "e"),
            ("revenue", "销售额", 105, "e"), ("ads", "全店广告", 95, "e"),
            ("platform", "预估佣金+收单", 120, "e"),
            ("purchase", "采购+预估运费", 120, "e"),
            ("profit", "实时预估利润", 115, "e"),
            ("settled", "Finance 已结算净额", 135, "e"),
        ])

    def _build_listing_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "跨境上品",
            "当前跨境 Seller API 可一键切换到上品工具；产品台账继续联动成本、重量和包装尺寸",
            include_filters=False,
        )
        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        top = tk.Frame(toolbar, bg="#FFFFFF")
        top.pack(fill="x", padx=16, pady=(11, 5))
        self.listing_guard_label = tk.Label(
            top, text="", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 9, "bold"),
        )
        self.listing_guard_label.pack(side="left")
        self.listing_sync_cost_button = self._button(
            top, "立即同步成本", self.sync_listing_ledger_costs, compact=True,
        )
        self.listing_sync_cost_button.pack(side="right")
        self.listing_open_tool_button = self._button(
            top, "打开上品工具", self.open_listing_tool, compact=True,
        )
        self.listing_open_tool_button.pack(side="right", padx=7)
        self.listing_connect_button = self._button(
            top, "连接 / 更换台账", self.configure_listing_ledger,
            secondary=True, compact=True,
        )
        self.listing_connect_button.pack(side="right")

        source_line = tk.Frame(toolbar, bg="#FFFFFF")
        source_line.pack(fill="x", padx=16, pady=(2, 11))
        self.listing_source_label = tk.Label(
            source_line, text="", justify="left", anchor="w", bg="#FFFFFF",
            fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        )
        self.listing_source_label.pack(side="left", fill="x", expand=True)
        self.listing_shop_label = tk.Label(
            source_line, text="", anchor="e", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        )
        self.listing_shop_label.pack(side="right")

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.listing_metric_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("shops", "台账内 Ozon 店铺", "个", PALETTE.primary),
            ("rows", "产品台账记录", "条", PALETTE.green),
            ("visible", "当前显示", "条", PALETTE.amber),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=5)
            cards.grid_columnconfigure(index, weight=1)
            self.listing_metric_labels[key] = card.value_label  # type: ignore[attr-defined]

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        header = tk.Frame(panel, bg="#FFFFFF")
        header.pack(fill="x", padx=16, pady=(12, 4))
        tk.Label(
            header, text="上品产品台账", bg="#FFFFFF", fg=PALETTE.text,
            font=("Microsoft YaHei UI", 11, "bold"),
        ).pack(side="left")
        self.listing_search_var = tk.StringVar()
        tk.Label(
            header, text="模糊搜索店铺 / 货号 / 商品 ID", bg="#FFFFFF",
            fg=PALETTE.muted,
        ).pack(side="left", padx=(25, 5))
        listing_search = ttk.Entry(header, width=34, textvariable=self.listing_search_var)
        listing_search.pack(side="left")
        listing_search.bind(
            "<KeyRelease>", lambda _event: self._debounced(
                "listing_query", lambda: self.refresh_listing_page(reload_data=False)
            ),
        )
        self._button(
            header, "重新读取台账", lambda: self.refresh_listing_page(reload_data=True),
            secondary=True, compact=True,
        ).pack(side="right")
        self.listing_ledger_tree = self._tree(panel, [
            ("shop", "上品店铺", 165, "w"), ("platform", "平台", 80, "w"),
            ("offer", "货号", 175, "w"), ("product_id", "平台商品 ID", 125, "w"),
            ("cost", "采购成本", 95, "e"), ("weight", "包装毛重 g", 100, "e"),
            ("dimensions", "包装长×宽×高 mm", 160, "e"),
            ("status", "状态", 160, "w"), ("updated", "更新时间", 150, "w"),
        ])
        self.refresh_listing_page(reload_data=True)

    def _build_weekly_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "周报", "每周一至周日汇总，并与前一完整周比较",
            include_filters=False,
        )
        bar = self._panel(page)
        bar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(bar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        latest_end = _latest_completed_week_end()
        self.weekly_end_var = tk.StringVar(value=latest_end.isoformat())
        self.weekly_auto_var = tk.BooleanVar(
            value=self.database.get_setting("weekly_auto_sync_enabled", "1") != "0"
        )
        tk.Label(controls, text="本期结束日（周日）", font=("Microsoft YaHei UI", 9, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        ttk.Entry(controls, width=12, textvariable=self.weekly_end_var).pack(side="left", padx=(8, 5))
        self._button(controls, "上一周", lambda: self._change_week(-7), secondary=True,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "下一周", lambda: self._change_week(7), secondary=True,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "最新完整周", self._select_latest_week, secondary=True,
                     compact=True).pack(side="left", padx=3)
        self._button(controls, "刷新报表", self.refresh_weekly, compact=True).pack(side="left", padx=(7, 3))
        self._button(controls, "立即拉取本周", self.sync_weekly_now, compact=True).pack(side="left", padx=3)
        tk.Checkbutton(
            controls, text="每周自动拉取", variable=self.weekly_auto_var,
            command=self._toggle_weekly_auto, bg="#FFFFFF", fg=PALETTE.text,
            activebackground="#FFFFFF", selectcolor="#FFFFFF",
            font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        summary_panel = self._panel(page)
        # Title + heading + three comparison rows + horizontal scrollbar need
        # roughly 260 px at Windows 125% display scaling.
        summary_panel.configure(height=265)
        summary_panel.pack(fill="x", padx=26, pady=(0, 10))
        summary_panel.pack_propagate(False)
        self._panel_title(summary_panel, "周度对比", "广告单占比 = 广告订单 ÷ 总销量；ACoTS = 广告花费 ÷ 总销售额")
        self.weekly_tree = self._tree(summary_panel, [
            ("period", "日期", 135, "w"), ("label", "期间", 65, "w"),
            ("revenue", "销售额", 110, "e"), ("orders", "销量", 80, "e"),
            ("spend", "广告花费", 100, "e"), ("ad_orders", "广告订单", 85, "e"),
            ("inventory", "库存", 75, "e"), ("ad_share", "广告单占比", 95, "e"),
            ("acots", "ACoTS", 75, "e"), ("sellable", "可售天数", 80, "e"),
        ])

        daily_panel = self._panel(page)
        daily_panel.pack(fill="both", expand=True, padx=26, pady=(0, 10))
        self._panel_title(daily_panel, "每日数据", "按自然日汇总本期数据")
        self.weekly_hint = tk.Label(
            daily_panel, text="", anchor="w", font=("Microsoft YaHei UI", 8),
            fg=PALETTE.muted, bg="#FFFFFF",
        )
        self.weekly_hint.pack(fill="x", padx=18, pady=(0, 2))
        self.weekly_daily_tree = self._tree(daily_panel, [
            ("day", "日期", 105, "w"), ("weekday", "星期", 70, "w"),
            ("revenue", "销售额", 110, "e"), ("orders", "销量", 75, "e"),
            ("spend", "广告花费", 100, "e"), ("ad_orders", "广告订单", 85, "e"),
            ("inventory", "库存", 75, "e"), ("ad_share", "广告单占比", 95, "e"),
            ("acots", "ACoTS", 75, "e"), ("sellable", "可售天数", 80, "e"),
        ])

    def _build_inventory_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "库存管理", "汇总查看全部商品；双击 SKU 打开集群库存与补货计划",
            include_filters=False,
        )
        self.inventory_product_var = tk.StringVar()
        self.inventory_search_var = tk.StringVar()
        self.inventory_target_days_var = tk.StringVar(value="30")
        self.inventory_planned_qty_var = tk.StringVar()
        self.inventory_status_var = tk.StringVar(
            value="双击任一商品，可查看全部集群并编辑计划补货数量。"
        )
        self._inventory_products_by_display: dict[str, dict[str, Any]] = {}
        self._inventory_products_by_iid: dict[str, dict[str, Any]] = {}
        self._inventory_rows_by_iid: dict[str, dict[str, Any]] = {}

        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        tk.Label(controls, text="搜索商品", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        search_entry = ttk.Entry(controls, width=38, textvariable=self.inventory_search_var)
        search_entry.pack(side="left", padx=(6, 12))
        search_entry.bind(
            "<KeyRelease>", lambda _event: self._debounced(
                "inventory_query", self.refresh_inventory_product_options
            ),
        )
        tk.Label(controls, text="目标可售天数", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        target_entry = ttk.Entry(controls, width=6, textvariable=self.inventory_target_days_var)
        target_entry.pack(side="left", padx=(6, 10))
        target_entry.bind("<Return>", lambda _event: self.refresh_inventory_product_options())
        self._button(
            controls, "刷新列表", self.refresh_inventory_product_options,
            secondary=True, compact=True,
        ).pack(side="left", padx=3)
        self._button(
            controls, "同步全部库存", self.sync_all_inventory, compact=True,
        ).pack(side="left", padx=3)

        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 10))
        self.inventory_overview_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("products", "商品数量", "个", "#7A5AF8"),
            ("available", "剩余库存", "件", PALETTE.primary),
            ("transit", "在途", "件", PALETTE.amber),
            ("daily", "日均销量", "件/天", PALETTE.green),
            ("monthly", "月估销量", "件", "#0BA5EC"),
            ("recommended", "建议补货", "件", "#F04438"),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=4)
            cards.grid_columnconfigure(index, weight=1)
            self.inventory_overview_labels[key] = card.value_label  # type: ignore[attr-defined]

        product_panel = self._panel(page)
        product_panel.pack(fill="both", expand=True, padx=26, pady=(0, 14))
        header = tk.Frame(product_panel, bg="#FFFFFF")
        header.pack(fill="x", padx=16, pady=(12, 4))
        tk.Label(header, text="全部商品库存", font=("Microsoft YaHei UI", 11, "bold"),
                 fg=PALETTE.text, bg="#FFFFFF").pack(side="left")
        tk.Label(
            header, text="双击商品行打开 SKU 详情与集群补货编辑窗口",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.primary, bg="#FFFFFF",
        ).pack(side="right")
        tk.Label(header, textvariable=self.inventory_status_var, font=("Microsoft YaHei UI", 8),
                 fg=PALETTE.muted, bg="#FFFFFF").pack(side="left", padx=14)
        self.inventory_product_tree = self._tree(product_panel, [
            ("offer", "货号", 155, "w"), ("sku", "Ozon SKU", 110, "w"),
            ("available", "剩余库存", 85, "e"), ("transit", "在途", 75, "e"),
            ("requested", "已申请", 75, "e"), ("daily", "日均销量", 85, "e"),
            ("monthly", "月估销量", 90, "e"),
            ("recommended", "建议补货", 90, "e"), ("planned", "计划补货", 90, "e"),
            ("clusters", "已计划集群", 90, "e"), ("days", "预计可售天数", 100, "e"),
        ])
        self.inventory_product_tree.bind("<Double-1>", self.open_inventory_product_detail)
        self.inventory_product_tree.bind("<Return>", self.open_inventory_product_detail)
        self.refresh_inventory_product_options()

    def _build_supply_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "约仓计划", "读取库存管理中保存的补货数量，规划直送或越库供应草稿",
            include_filters=False,
        )
        self.supply_plan_product_var = tk.StringVar()
        self.supply_plan_type_var = tk.StringVar(value="直送")
        self.supply_plan_warehouse_var = tk.StringVar()
        self.supply_plan_status_var = tk.StringVar(
            value="选择已保存补货量的集群，可创建并校验新的 Ozon 约仓草稿。"
        )
        self._supply_products_by_display: dict[str, dict[str, Any]] = {}
        self._supply_plan_rows: list[dict[str, Any]] = []
        self._supply_plan_rows_by_iid: dict[str, dict[str, Any]] = {}
        self._supply_warehouses_by_display: dict[str, dict[str, Any]] = {}

        toolbar = self._panel(page)
        toolbar.pack(fill="x", padx=26, pady=(0, 10))
        controls = tk.Frame(toolbar, bg="#FFFFFF")
        controls.pack(fill="x", padx=16, pady=10)
        tk.Label(controls, text="商品", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        self.supply_plan_product_combo = ttk.Combobox(
            controls, state="normal", width=43, textvariable=self.supply_plan_product_var,
        )
        self.supply_plan_product_combo.pack(side="left", padx=(6, 12))
        self.supply_plan_product_combo.bind("<KeyRelease>", self._filter_supply_products)
        self.supply_plan_product_combo.bind(
            "<<ComboboxSelected>>", lambda _event: self.refresh_supply_plan()
        )
        self.supply_plan_product_combo.bind("<Return>", lambda _event: self.refresh_supply_plan())
        tk.Label(controls, text="供应方式", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        self._supply_type_values = ("直送", "越库")
        self.supply_plan_type_combo = ttk.Combobox(
            controls, state="normal", width=8, textvariable=self.supply_plan_type_var,
            values=self._supply_type_values,
        )
        self.supply_plan_type_combo.pack(side="left", padx=(6, 12))
        self.supply_plan_type_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.supply_plan_type_combo, self._supply_type_values,
                self.supply_plan_type_var,
            ),
        )
        self.supply_plan_type_combo.bind(
            "<<ComboboxSelected>>", lambda _event: self.refresh_supply_plan()
        )
        self._button(controls, "刷新计划", self.refresh_supply_plan, secondary=True, compact=True).pack(side="left")
        self._button(
            controls, "返回库存设置数量", lambda: self.show_page("inventory"),
            secondary=True, compact=True,
        ).pack(side="right")

        summary = tk.Frame(page, bg=PALETTE.bg)
        summary.pack(fill="x", padx=26, pady=(0, 10))
        self.supply_plan_summary_labels: dict[str, tk.Label] = {}
        for index, (key, title, unit, color) in enumerate([
            ("clusters", "补货集群", "个", PALETTE.primary),
            ("quantity", "计划补货", "件", PALETTE.green),
            ("available", "当前库存", "件", PALETTE.amber),
            ("after_days", "整体补货后可售", "天", "#7A5AF8"),
        ]):
            card = self._metric_card(summary, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=5)
            summary.grid_columnconfigure(index, weight=1)
            self.supply_plan_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 10))
        self._panel_title(
            panel, "待约仓集群", "仅显示库存管理中已保存且计划补货量大于 0 的集群",
        )
        self.supply_plan_tree = self._tree(panel, [
            ("cluster", "集群（俄文）", 190, "w"),
            ("cluster_cn", "集群中文名", 155, "w"), ("macro", "集群 ID", 90, "w"),
            ("available", "当前库存", 85, "e"), ("transit", "在途", 75, "e"),
            ("requested", "已申请", 75, "e"), ("daily", "日均销量", 85, "e"),
            ("planned", "计划补货", 90, "e"), ("days", "补货后可售天数", 120, "e"),
        ])
        self.supply_plan_tree.bind("<Double-1>", self.open_supply_api_wizard)

        action_panel = self._panel(page)
        action_panel.pack(fill="x", padx=26, pady=(0, 14))
        row = tk.Frame(action_panel, bg="#FFFFFF")
        row.pack(fill="x", padx=16, pady=(10, 4))
        tk.Label(row, text="发货地址", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        self.supply_plan_warehouse_combo = ttk.Combobox(
            row, state="normal", width=45, textvariable=self.supply_plan_warehouse_var,
        )
        self.supply_plan_warehouse_combo.pack(side="left", padx=(6, 10))
        self.supply_plan_warehouse_combo.bind("<KeyRelease>", self._filter_supply_warehouses)
        self.supply_plan_warehouse_combo.bind(
            "<<ComboboxSelected>>", self._supply_warehouse_selected
        )
        self._button(
            row, "查看本地草稿", self.preview_supply_draft, secondary=True, compact=True,
        ).pack(side="right")
        self._button(
            row, "创建 Ozon 约仓", self.open_supply_api_wizard, compact=True,
        ).pack(side="right")
        self.supply_plan_address_label = tk.Label(
            action_panel, text="发货地址将在同步库存后显示。", anchor="w", bg="#FFFFFF",
            fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        )
        self.supply_plan_address_label.pack(fill="x", padx=16, pady=(0, 4))
        tk.Label(
            action_panel, textvariable=self.supply_plan_status_var, anchor="w", bg="#FFFFFF",
            fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(fill="x", padx=16, pady=(0, 9))
        self.refresh_supply_product_options()

    def _build_placeholder_page(
        self, page: tk.Frame, title: str, subtitle: str, message: str,
    ) -> None:
        self._page_header(page, title, subtitle)
        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=26, pady=(0, 20))
        body = tk.Frame(panel, bg="#FFFFFF")
        body.place(relx=0.5, rely=0.43, anchor="center")
        tk.Label(
            body, text="功能框架已建立", font=("Microsoft YaHei UI", 18, "bold"),
            fg=PALETTE.text, bg="#FFFFFF",
        ).pack()
        tk.Label(
            body, text=message, wraplength=620, justify="center",
            font=("Microsoft YaHei UI", 10), fg=PALETTE.muted, bg="#FFFFFF",
        ).pack(pady=(12, 0))

    def _build_sync_page(self, page: tk.Frame) -> None:
        self._page_header(page, "数据中心", "手动同步、导出演示数据和查看运行日志", include_filters=True)
        action_panel = self._panel(page)
        action_panel.pack(fill="x", padx=26, pady=(0, 14))
        actions = tk.Frame(action_panel, bg="#FFFFFF")
        actions.pack(fill="x", padx=18, pady=17)
        self._button(actions, "同步全部数据", lambda: self.start_sync("all")).pack(side="left")
        self._button(actions, "仅同步 Seller", lambda: self.start_sync("seller"), secondary=True).pack(side="left", padx=8)
        self._button(actions, "快速同步广告", lambda: self.start_sync("performance"), secondary=True).pack(side="left")
        self._button(actions, "生成演示数据", self.seed_demo, secondary=True).pack(side="left", padx=(24, 8))
        self._button(actions, "删除演示数据", self.delete_demo, secondary=True).pack(side="left")
        self._button(actions, "导出 CSV", self.export_csv, secondary=True).pack(side="left")
        self.cancel_button = self._button(actions, "取消同步", self.cancel_sync, secondary=True)
        self.cancel_button.pack(side="left", padx=(8, 0))
        self.cancel_button.configure(state="disabled")
        tk.Label(actions, text="提示：首次体验可先生成演示数据", font=("Microsoft YaHei UI", 8),
                 fg=PALETTE.muted, bg="#FFFFFF").pack(side="right")

        log_panel = self._panel(page)
        log_panel.pack(fill="both", expand=True, padx=26, pady=(0, 20))
        self._panel_title(log_panel, "同步日志", "选择一条任务，可查看请求、返回、解析和写库的完整过程")
        log_split = tk.PanedWindow(
            log_panel, orient="vertical", bg=PALETTE.border, bd=0,
            sashwidth=5, sashrelief="flat",
        )
        log_split.pack(fill="both", expand=True, padx=15, pady=(0, 15))

        summary_panel = tk.Frame(log_split, bg="#FFFFFF")
        detail_panel = tk.Frame(log_split, bg="#FFFFFF")
        log_split.add(summary_panel, minsize=145, height=245)
        log_split.add(detail_panel, minsize=150)
        columns = [
            ("started_at", "开始时间", 145, "w"), ("source", "数据源", 140, "w"),
            ("status", "状态", 85, "center"), ("rows_count", "数据行", 80, "e"),
            ("message", "说明", 420, "w"),
        ]
        self.log_tree = self._tree(summary_panel, columns)
        self.log_tree.bind("<<TreeviewSelect>>", self._log_selected)

        detail_header = tk.Frame(detail_panel, bg="#FFFFFF")
        detail_header.pack(fill="x", padx=2, pady=(4, 5))
        self.log_detail_title = tk.Label(
            detail_header, text="运行明细", font=("Microsoft YaHei UI", 9, "bold"),
            fg=PALETTE.text, bg="#FFFFFF",
        )
        self.log_detail_title.pack(side="left")
        self._button(
            detail_header, "复制明细", self.copy_log_details, secondary=True, compact=True
        ).pack(side="right")

        detail_holder = tk.Frame(detail_panel, bg="#FFFFFF")
        detail_holder.pack(fill="both", expand=True, padx=2)
        self.log_detail_text = tk.Text(
            detail_holder, wrap="word", relief="flat", bd=0, bg="#F8FAFD",
            fg="#344054", font=("Consolas", 9), padx=10, pady=8,
            state="disabled",
        )
        detail_scrollbar = ttk.Scrollbar(
            detail_holder, orient="vertical", command=self.log_detail_text.yview
        )
        self.log_detail_text.configure(yscrollcommand=detail_scrollbar.set)
        detail_scrollbar.pack(side="right", fill="y")
        self.log_detail_text.pack(side="left", fill="both", expand=True)

    def _build_settings_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "API 配置中心",
            "每个命名配置只服务一个店铺；本土店与跨境店的数据和密钥完全隔离",
            include_filters=False,
        )
        shop_context = self._panel(page)
        shop_context.pack(fill="x", padx=26, pady=(0, 10))
        profile_line = tk.Frame(shop_context, bg="#FFFFFF")
        profile_line.pack(fill="x", padx=16, pady=(12, 5))
        tk.Label(
            profile_line, text="API 配置", bg="#FFFFFF", fg=PALETTE.text,
            font=("Microsoft YaHei UI", 9, "bold"),
        ).pack(side="left")
        self.api_profile_var = tk.StringVar(value=self.active_shop.api_display_name)
        self.api_profile_combo = ttk.Combobox(
            profile_line, state="normal", width=43, textvariable=self.api_profile_var,
        )
        self.api_profile_combo.pack(side="left", padx=(10, 7))
        self.api_profile_combo.bind(
            "<<ComboboxSelected>>", self._switch_api_profile_from_combo,
        )
        self.api_profile_combo.bind("<Return>", self._switch_api_profile_from_combo)
        self.api_profile_combo.bind("<KeyRelease>", self._filter_api_profile_combo)
        self._button(
            profile_line, "切换配置", self._switch_api_profile_from_combo,
            secondary=True, compact=True,
        ).pack(side="left", padx=(0, 7))
        self._button(
            profile_line, "+ 新增 API 配置",
            lambda: self.open_shop_manager(create_mode=True), compact=True,
        ).pack(side="left")
        self._button(
            profile_line, "管理 / 重命名", self.open_shop_manager,
            secondary=True, compact=True,
        ).pack(side="right")

        shop_line = tk.Frame(shop_context, bg="#FFFFFF")
        shop_line.pack(fill="x", padx=16, pady=(3, 11))
        self.settings_type_badge = tk.Label(
            shop_line, text=self.active_shop.kind_label, bg="#EFF4FF",
            fg="#B54708" if self.active_shop.kind == "cross_border" else PALETTE.primary,
            font=("Microsoft YaHei UI", 8, "bold"), padx=9, pady=4,
        )
        self.settings_type_badge.pack(side="left", padx=(0, 9))
        self.settings_shop_label = tk.Label(
            shop_line,
            text=(
                f"当前 API：{self.active_shop.resolved_api_name}  ｜  "
                f"店铺：{self.active_shop.name}  ｜  类型：{self.active_shop.kind_label}  ｜  "
                "独立 API / 独立数据库"
            ),
            bg="#FFFFFF", fg=PALETTE.text, font=("Microsoft YaHei UI", 9, "bold"),
        )
        self.settings_shop_label.pack(side="left")
        tk.Label(
            shop_line,
            text="类型创建后不可修改，避免把本土数据写进跨境数据库；类型选错请新增配置。",
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(side="left", padx=18)
        self._refresh_shop_selector()
        container = tk.Frame(page, bg=PALETTE.bg)
        container.pack(fill="both", expand=True, padx=26, pady=(0, 20))

        credentials = self.service.load_credentials()
        self.credential_vars = {
            "seller_client_id": tk.StringVar(value=credentials.seller_client_id),
            "seller_api_key": tk.StringVar(value=credentials.seller_api_key),
            "performance_client_id": tk.StringVar(value=credentials.performance_client_id),
            "performance_client_secret": tk.StringVar(value=credentials.performance_client_secret),
            "ai_base_url": tk.StringVar(value=credentials.ai_base_url),
            "ai_api_key": tk.StringVar(value=credentials.ai_api_key),
            "ai_model": tk.StringVar(value=credentials.ai_model),
            "feishu_base_url": tk.StringVar(value=credentials.feishu_base_url),
            "feishu_app_id": tk.StringVar(value=credentials.feishu_app_id),
            "feishu_app_secret": tk.StringVar(value=credentials.feishu_app_secret),
            "feishu_app_token": tk.StringVar(value=credentials.feishu_app_token),
            "feishu_product_table_id": tk.StringVar(value=credentials.feishu_product_table_id),
            "feishu_weekly_table_id": tk.StringVar(value=credentials.feishu_weekly_table_id),
            "feishu_tracking_table_id": tk.StringVar(value=credentials.feishu_tracking_table_id),
            "feishu_series_table_id": tk.StringVar(value=credentials.feishu_series_table_id),
            "feishu_chat_id": tk.StringVar(value=credentials.feishu_chat_id),
        }
        seller = self._settings_card(
            container, f"{self.active_shop.kind_label} Seller API",
            "只写入当前命名店铺的销量、订单、库存和财务数据库",
        )
        self.settings_seller_title_label = seller.title_label  # type: ignore[attr-defined]
        seller.grid(row=0, column=0, sticky="nsew", padx=(0, 5))
        self._form_field(seller.body, "Seller Client ID", self.credential_vars["seller_client_id"])  # type: ignore[attr-defined]
        self._form_field(seller.body, "Seller API Key", self.credential_vars["seller_api_key"], secret=True)  # type: ignore[attr-defined]
        tk.Label(seller.body, text="后台：设置 → API 密钥 → Seller API", bg="#FFFFFF",  # type: ignore[attr-defined]
                 fg=PALETTE.muted, font=("Microsoft YaHei UI", 8)).pack(anchor="w", pady=(0, 14))
        self._button(seller.body, "测试 Seller 连接", self.test_seller, secondary=True).pack(anchor="w")  # type: ignore[attr-defined]

        performance = self._settings_card(container, "Performance API", "广告活动与投放统计数据")
        performance.grid(row=0, column=1, sticky="nsew", padx=5)
        self._form_field(performance.body, "Performance Client ID", self.credential_vars["performance_client_id"])  # type: ignore[attr-defined]
        self._form_field(performance.body, "Performance Client Secret", self.credential_vars["performance_client_secret"], secret=True)  # type: ignore[attr-defined]
        tk.Label(performance.body, text="后台：设置 → API 密钥 → Performance API", bg="#FFFFFF",  # type: ignore[attr-defined]
                 fg=PALETTE.muted, font=("Microsoft YaHei UI", 8)).pack(anchor="w", pady=(0, 14))
        self._button(performance.body, "测试广告 API", self.test_performance, secondary=True).pack(anchor="w")  # type: ignore[attr-defined]

        ai_card = self._settings_card(container, "AI 中转站", "OpenAI 兼容的 Chat Completions API")
        ai_card.grid(row=0, column=2, sticky="nsew", padx=(5, 0))
        self._form_field(ai_card.body, "Base URL", self.credential_vars["ai_base_url"])  # type: ignore[attr-defined]
        self._form_field(ai_card.body, "API Key", self.credential_vars["ai_api_key"], secret=True)  # type: ignore[attr-defined]
        self._form_field(ai_card.body, "Model", self.credential_vars["ai_model"])  # type: ignore[attr-defined]
        tk.Label(ai_card.body, text="例：https://api.gpt.ge/v1", bg="#FFFFFF",  # type: ignore[attr-defined]
                 fg=PALETTE.muted, font=("Microsoft YaHei UI", 8)).pack(anchor="w", pady=(0, 14))
        self._button(ai_card.body, "测试 AI 连接", self.test_ai, secondary=True).pack(anchor="w")  # type: ignore[attr-defined]
        for column in range(3):
            container.grid_columnconfigure(column, weight=1, uniform="settings")
        container.grid_rowconfigure(0, weight=1)

        footer = tk.Frame(page, bg=PALETTE.bg)
        footer.pack(fill="x", padx=26, pady=(0, 25))
        tk.Label(footer, text="密钥加密保存在本机；导出的迁移文件包含明文密钥，请妥善保管。",
                 bg=PALETTE.bg, fg=PALETTE.muted, font=("Microsoft YaHei UI", 8)).pack(side="left")
        self._button(footer, "保存设置", self.save_credentials).pack(side="right")
        self._button(
            footer, "导入 API 配置", self.import_api_profiles,
            secondary=True, compact=True,
        ).pack(side="right", padx=5)
        self._button(
            footer, "导出全部 API", self.export_api_profiles,
            secondary=True, compact=True,
        ).pack(side="right")

    def _build_feishu_page(self, page: tk.Frame) -> None:
        self._page_header(
            page, "飞书同步", "商品成本双向同步，并在每周数据更新后自动发送周报",
            include_filters=False,
        )
        config_panel = self._panel(page)
        config_panel.pack(fill="x", padx=26, pady=(0, 12))
        self._panel_title(
            config_panel, "飞书应用与目标位置",
            "使用企业自建应用；App Secret 会通过 Windows DPAPI 加密保存在本机",
        )
        form = tk.Frame(config_panel, bg="#FFFFFF")
        form.pack(fill="x", padx=18, pady=(0, 14))

        def field(
            row: int, column: int, title: str, key: str, *, secret: bool = False,
        ) -> None:
            cell = tk.Frame(form, bg="#FFFFFF")
            cell.grid(row=row, column=column, sticky="ew", padx=7, pady=5)
            tk.Label(
                cell, text=title, bg="#FFFFFF", fg=PALETTE.text,
                font=("Microsoft YaHei UI", 8, "bold"),
            ).pack(anchor="w", pady=(0, 4))
            ttk.Entry(
                cell, textvariable=self.credential_vars[key], show="•" if secret else "",
            ).pack(fill="x")

        field(0, 0, "App ID", "feishu_app_id")
        field(0, 1, "App Secret", "feishu_app_secret", secret=True)
        field(0, 2, "多维表格 App Token", "feishu_app_token")
        field(1, 0, "商品表 Table ID", "feishu_product_table_id")
        field(1, 1, "周报表 Table ID", "feishu_weekly_table_id")
        field(1, 2, "发货跟踪 Table ID", "feishu_tracking_table_id")
        field(2, 0, "接收报表、产品系列与到库通知的群 Chat ID", "feishu_chat_id")
        for column in range(3):
            form.grid_columnconfigure(column, weight=1, uniform="feishu")
        config_actions = tk.Frame(config_panel, bg="#FFFFFF")
        config_actions.pack(fill="x", padx=25, pady=(0, 16))
        self._button(
            config_actions, "保存飞书设置", self.save_feishu_credentials, compact=True,
        ).pack(side="left")
        self._button(
            config_actions, "测试连接", self.test_feishu, secondary=True, compact=True,
        ).pack(side="left", padx=7)
        tk.Label(
            config_actions,
            text=(
                "表格链接通常包含 /base/{App Token}?table={Table ID}；"
                "产品系列统计使用 Chat ID 手动发群，机器人需先加入目标群。"
            ),
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        actions = tk.Frame(page, bg=PALETTE.bg)
        actions.pack(fill="both", expand=True, padx=26, pady=(0, 12))
        actions.grid_columnconfigure(0, weight=1, uniform="feishu-actions")
        actions.grid_columnconfigure(1, weight=1, uniform="feishu-actions")
        actions.grid_rowconfigure(0, weight=1)

        product = self._panel(actions)
        product.grid(row=0, column=0, sticky="nsew", padx=(0, 6))
        self._panel_title(
            product, "商品成本双向同步",
            "SKU 为唯一键；飞书成本先写入本地，再把本地商品增量补回飞书",
        )
        product_buttons = tk.Frame(product, bg="#FFFFFF")
        product_buttons.pack(fill="x", padx=18, pady=(8, 7))
        self._button(
            product_buttons, "立即双向同步", lambda: self.start_feishu_product_sync("both"),
        ).pack(side="left")
        self._button(
            product_buttons, "仅从飞书读取", lambda: self.start_feishu_product_sync("pull"),
            secondary=True,
        ).pack(side="left", padx=7)
        self._button(
            product_buttons, "仅上传到飞书", lambda: self.start_feishu_product_sync("push"),
            secondary=True,
        ).pack(side="left")
        tk.Label(
            product,
            text="自动创建字段：SKU、货号、商品名称、单件成本、单件头程、备注、本地更新时间。\n"
                 "只上传新增或有变化的数据，不会重复创建相同 SKU。",
            justify="left", wraplength=560, bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=18, pady=(10, 16))

        weekly = self._panel(actions)
        weekly.grid(row=0, column=1, sticky="nsew", padx=(6, 0))
        self._panel_title(
            weekly, "每周自动发送周报",
            "Ozon 周报拉取完成后，更新多维表格并向指定飞书群发送摘要",
        )
        weekly_buttons = tk.Frame(weekly, bg="#FFFFFF")
        weekly_buttons.pack(fill="x", padx=18, pady=(8, 7))
        self._button(
            weekly_buttons, "写入周报表并发群",
            lambda: self.start_feishu_weekly_sync(True, True),
        ).pack(side="left")
        self._button(
            weekly_buttons, "仅写入表格",
            lambda: self.start_feishu_weekly_sync(True, False),
            secondary=True,
        ).pack(side="left", padx=7)
        self._button(
            weekly_buttons, "仅发送到群",
            lambda: self.start_feishu_weekly_sync(False, True),
            secondary=True,
        ).pack(side="left")
        tk.Frame(weekly, bg=PALETTE.border, height=1).pack(
            fill="x", padx=18, pady=(11, 8),
        )
        tk.Label(
            weekly, text="其他经营报表", bg="#FFFFFF", fg=PALETTE.text,
            font=("Microsoft YaHei UI", 9, "bold"),
        ).pack(anchor="w", padx=18)
        report_buttons = tk.Frame(weekly, bg="#FFFFFF")
        report_buttons.pack(fill="x", padx=18, pady=(7, 3))
        current_month = date.today().strftime("%Y-%m")
        self.feishu_month_var = tk.StringVar(value=current_month)
        tk.Label(
            report_buttons, text="月份", bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left")
        ttk.Entry(
            report_buttons, width=9, textvariable=self.feishu_month_var,
        ).pack(side="left", padx=(5, 6))
        self._button(
            report_buttons, "发送月度盈亏表", self.start_feishu_monthly_profit,
            secondary=True, compact=True,
        ).pack(side="left")
        tk.Label(
            report_buttons, text="跨境周期", bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left", padx=(16, 4))
        self.feishu_cross_from_var = tk.StringVar(value=self.date_from_var.get())
        self.feishu_cross_to_var = tk.StringVar(value=self.date_to_var.get())
        ttk.Entry(
            report_buttons, width=11, textvariable=self.feishu_cross_from_var,
        ).pack(side="left")
        tk.Label(
            report_buttons, text="至", bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left", padx=4)
        ttk.Entry(
            report_buttons, width=11, textvariable=self.feishu_cross_to_var,
        ).pack(side="left")
        self._button(
            report_buttons, "发送跨境周利润", self.start_feishu_cross_border_profit,
            secondary=True, compact=True,
        ).pack(side="left", padx=(6, 0))
        self.feishu_weekly_auto_var = tk.BooleanVar(
            value=self.database.get_setting("feishu_weekly_auto_enabled", "1") != "0"
        )
        tk.Checkbutton(
            weekly, text="每周数据更新成功后自动写表并发群",
            variable=self.feishu_weekly_auto_var,
            command=self._toggle_feishu_weekly_auto,
            bg="#FFFFFF", fg=PALETTE.text, activebackground="#FFFFFF",
            selectcolor="#FFFFFF", font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=18, pady=(8, 4))
        scheduler_buttons = tk.Frame(weekly, bg="#FFFFFF")
        scheduler_buttons.pack(fill="x", padx=18, pady=(4, 3))
        self._button(
            scheduler_buttons, "安装后台计划任务", self.install_feishu_schedule,
            secondary=True, compact=True,
        ).pack(side="left")
        self._button(
            scheduler_buttons, "移除计划任务", self.remove_feishu_schedule,
            secondary=True, compact=True,
        ).pack(side="left", padx=7)
        tk.Label(
            weekly, text="计划任务每天 09:00 检查；同一周自动任务只发送一次。",
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=18, pady=(0, 16))

        status_panel = self._panel(page)
        status_panel.pack(fill="x", padx=26, pady=(0, 20))
        self.feishu_status_var = tk.StringVar(value="等待配置飞书应用")
        tk.Label(
            status_panel, textvariable=self.feishu_status_var, anchor="w",
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(fill="x", padx=16, pady=10)

    def _metric_card(self, parent: tk.Widget, title: str, unit: str, color: str, small: bool = False) -> tk.Frame:
        card = tk.Frame(
            parent, bg=PALETTE.panel, highlightbackground=PALETTE.border,
            highlightthickness=1, height=116 if not small else 98,
        )
        card.grid_propagate(False)
        body = tk.Frame(card, bg=PALETTE.panel)
        body.pack(fill="both", expand=True, padx=20, pady=14 if small else 17)
        title_line = tk.Frame(body, bg=PALETTE.panel)
        title_line.pack(fill="x")
        dot = tk.Frame(title_line, bg=color, width=9, height=9)
        dot.pack(side="left", padx=(0, 8))
        dot.pack_propagate(False)
        tk.Label(
            title_line, text=title, font=("Microsoft YaHei UI", 9),
            fg=PALETTE.muted, bg=PALETTE.panel,
        ).pack(side="left")
        line = tk.Frame(body, bg=PALETTE.panel)
        line.pack(anchor="w", pady=(9, 0))
        value = tk.Label(
            line, text="--", font=("Segoe UI", 22 if not small else 18, "bold"),
            fg=PALETTE.text, bg=PALETTE.panel,
        )
        value.pack(side="left")
        unit_label = tk.Label(line, text=unit, font=("Microsoft YaHei UI", 8), fg=PALETTE.muted,
                              bg=PALETTE.panel)
        unit_label.pack(side="left", padx=(6, 0), pady=(9, 0))
        card.value_label = value  # type: ignore[attr-defined]
        card.unit_label = unit_label  # type: ignore[attr-defined]
        if unit == "₽":
            self._currency_unit_labels.append(unit_label)
        return card

    def _bind_click_tree(
        self, widget: tk.Widget, callback: Callable[[tk.Event[Any]], None],
    ) -> None:
        """Make a compound Tk card behave as one clickable control."""
        try:
            widget.configure(cursor="hand2")
        except tk.TclError:
            pass
        widget.bind("<Button-1>", callback)
        for child in widget.winfo_children():
            self._bind_click_tree(child, callback)

    def _settings_card(self, parent: tk.Widget, title: str, subtitle: str) -> tk.Frame:
        card = self._panel(parent)
        head = tk.Frame(card, bg="#FFFFFF")
        head.pack(fill="x", padx=22, pady=(20, 10))
        title_label = tk.Label(
            head, text=title, font=("Microsoft YaHei UI", 14, "bold"),
            fg=PALETTE.text, bg="#FFFFFF",
        )
        title_label.pack(anchor="w")
        tk.Label(head, text=subtitle, font=("Microsoft YaHei UI", 9), fg=PALETTE.muted,
                 bg="#FFFFFF").pack(anchor="w", pady=(2, 0))
        body = tk.Frame(card, bg="#FFFFFF")
        body.pack(fill="both", expand=True, padx=22, pady=(5, 22))
        card.body = body  # type: ignore[attr-defined]
        card.title_label = title_label  # type: ignore[attr-defined]
        return card

    def _form_field(self, parent: tk.Widget, label: str, variable: tk.StringVar, secret: bool = False) -> None:
        tk.Label(parent, text=label, font=("Microsoft YaHei UI", 9), fg=PALETTE.text,
                 bg="#FFFFFF").pack(anchor="w", pady=(10, 5))
        ttk.Entry(parent, textvariable=variable, show="•" if secret else "").pack(fill="x", ipady=3)

    def _panel(self, parent: tk.Widget, width: int | None = None) -> tk.Frame:
        kwargs: dict[str, Any] = {
            "bg": PALETTE.panel, "highlightbackground": PALETTE.border,
            "highlightthickness": 1, "bd": 0, "relief": "flat",
        }
        if width:
            kwargs["width"] = width
        return tk.Frame(parent, **kwargs)

    def _panel_title(self, parent: tk.Widget, title: str, subtitle: str) -> None:
        block = tk.Frame(parent, bg="#FFFFFF")
        block.pack(fill="x", padx=18, pady=(15, 10))
        tk.Label(block, text=title, font=("Microsoft YaHei UI", 11, "bold"), fg=PALETTE.text,
                 bg="#FFFFFF").pack(anchor="w")
        tk.Label(block, text=subtitle, font=("Microsoft YaHei UI", 8), fg=PALETTE.muted,
                 bg="#FFFFFF").pack(anchor="w", pady=(2, 0))

    def _button(self, parent: tk.Widget, text: str, command: Callable[[], None],
                secondary: bool = False, compact: bool = False) -> tk.Button:
        bg = "#F4F6FA" if secondary else PALETTE.primary
        fg = PALETTE.sidebar_text if secondary else "#FFFFFF"
        active_bg = "#E9EDF5" if secondary else PALETTE.primary_dark
        button = tk.Button(
            parent, text=text, command=command, cursor="hand2", relief="flat", bd=0,
            bg=bg, fg=fg, activebackground=active_bg, activeforeground=fg,
            highlightthickness=0,
            font=("Microsoft YaHei UI", 8 if compact else 9, "bold"),
            padx=14 if compact else 19, pady=8 if compact else 10,
        )
        button.bind("<Enter>", lambda _event: button.configure(bg=active_bg))
        button.bind("<Leave>", lambda _event: button.configure(bg=bg))
        return button

    def _tree(self, parent: tk.Widget, columns: list[tuple[str, str, int, str]]) -> ttk.Treeview:
        holder = tk.Frame(parent, bg=PALETTE.panel)
        holder.pack(fill="both", expand=True, padx=16, pady=(6, 16))
        tree = ttk.Treeview(holder, columns=[column[0] for column in columns], show="headings")
        for key, label, width, anchor in columns:
            tree.heading(key, text=label)
            tree.column(key, width=width, minwidth=55, anchor=anchor, stretch=key in {"product_name", "campaign_name", "message"})
        scrollbar = ttk.Scrollbar(holder, orient="vertical", command=tree.yview)
        horizontal = ttk.Scrollbar(holder, orient="horizontal", command=tree.xview)
        tree.configure(yscrollcommand=scrollbar.set, xscrollcommand=horizontal.set)
        holder.grid_rowconfigure(0, weight=1)
        holder.grid_columnconfigure(0, weight=1)
        tree.grid(row=0, column=0, sticky="nsew")
        scrollbar.grid(row=0, column=1, sticky="ns")
        horizontal.grid(row=1, column=0, sticky="ew")
        tree.tag_configure("odd", background=PALETTE.panel_alt)
        return tree

    def _nav_hover(self, key: str, entering: bool) -> None:
        if key == self.current_page:
            return
        color = PALETTE.sidebar_hover if entering else PALETTE.sidebar
        self.nav_holders[key].configure(bg=color)
        self.nav_buttons[key].configure(bg=color)

    def _toggle_nav_category(self, key: str, *, force_open: bool = False) -> None:
        """Show one navigation group at a time so the sidebar stays compact."""
        if key not in self.nav_category_frames:
            return
        target = key if force_open or self.nav_expanded_category != key else None
        self.nav_expanded_category = target
        active_category = self.nav_page_categories.get(self.current_page)
        for category, frame in self.nav_category_frames.items():
            expanded = category == target
            if expanded:
                frame.pack(fill="x")
            else:
                frame.pack_forget()
            label = self.nav_category_labels[category]
            selected = category == active_category
            self.nav_category_buttons[category].configure(
                text=f"{'▾' if expanded else '▸'}  {label}",
                bg=PALETTE.sidebar_hover if selected or expanded else PALETTE.sidebar,
                fg=PALETTE.primary_dark if selected else PALETTE.sidebar_muted,
            )

    def _apply_shop_page_access(self) -> None:
        """Keep local-tax reports out of cross-border shop workspaces."""
        disabled = self.active_shop.kind == "cross_border"
        for key in ("profit", "product_profit", "product_daily"):
            button = self.nav_buttons.get(key)
            if button is not None:
                button.configure(
                    state="disabled" if disabled else "normal",
                    cursor="arrow" if disabled else "hand2",
                )

    def _debounced(self, key: str, callback: Callable[[], None], delay: int = 220) -> None:
        previous = self._search_after_ids.pop(key, None)
        if previous:
            try:
                self.after_cancel(previous)
            except tk.TclError:
                pass
        self._search_after_ids[key] = self.after(
            delay, lambda: (self._search_after_ids.pop(key, None), callback())
        )

    def _schedule_page_refresh(self, key: str, *, force: bool = False) -> None:
        if not force and key not in self._dirty_pages:
            return
        if self._refresh_after_id:
            try:
                self.after_cancel(self._refresh_after_id)
            except tk.TclError:
                pass
        self._refresh_after_id = self.after_idle(lambda: self._refresh_page_now(key))

    def _refresh_page_now(self, key: str) -> None:
        self._refresh_after_id = None
        if key != self.current_page:
            return
        self._dirty_pages.discard(key)
        callbacks: dict[str, Callable[[], None]] = {
            "dashboard": self._refresh_dashboard_data,
            "advertising": self.refresh_advertising,
            "sales": self.refresh_sales,
            "ai": self.refresh_ai_product_options,
            "weekly": self.refresh_weekly,
            "profit": self.refresh_profit,
            "product_profit": self.refresh_product_profit,
            "orders": self.refresh_orders,
            "product_daily": self.refresh_product_daily,
            "series": self.refresh_product_series,
            "costs": self.refresh_costs,
            "cross_border": self.refresh_cross_border,
            "listing": lambda: self.refresh_listing_page(reload_data=True),
            "inventory": self.refresh_inventory_product_options,
            "supply": self.refresh_supply_product_options,
            "sync": self.refresh_logs,
            "shipment_tracking": self.refresh_shipment_tracking,
        }
        callback = callbacks.get(key)
        if callback:
            callback()

    def _refresh_dashboard_data(self) -> None:
        date_from, date_to = self._date_range()
        self.summary = self.database.dashboard_summary(date_from, date_to)
        self.trend_rows = self.database.daily_trend(date_from, date_to)
        self._refresh_dashboard()
        cache = self.database.sales_sync_state(date_from, date_to)
        cache_text = ""
        if int(cache.get("rows_count") or 0):
            max_day = str(cache.get("max_day") or "")
            verified = str(cache.get("verified_through") or "")
            if verified and verified >= date_to:
                cache_text = f"；Seller 已核验至 {verified}"
            elif max_day:
                cache_text = f"；Seller 缓存截至 {max_day}（末日数据待补齐/核验）"
        self.status_var.set(f"已加载 {date_from} 至 {date_to} 的本地数据{cache_text}")

    def show_page(self, key: str) -> None:
        if key in {"profit", "product_profit", "product_daily"} and self.active_shop.kind == "cross_border":
            messagebox.showinfo(
                "仅限本土店",
                "月度盈亏表、产品预测、订单明细和每日产品经营仅对本土店开放；请切换本土店 API 配置。",
                parent=self,
            )
            return
        category = self.nav_page_categories.get(key)
        if category:
            self._toggle_nav_category(category, force_open=True)
        self.current_page = key
        self.page_frames[key].tkraise()
        for page, button in self.nav_buttons.items():
            selected = page == key
            background = PALETTE.sidebar_active if selected else PALETTE.sidebar
            button.configure(
                bg=background,
                fg=PALETTE.primary_dark if selected else PALETTE.sidebar_text,
                font=("Microsoft YaHei UI", 9, "bold" if selected else "normal"),
            )
            self.nav_holders[page].configure(bg=background)
            self.nav_accents[page].configure(
                bg=PALETTE.primary if selected else background
            )
        if category:
            self._toggle_nav_category(category, force_open=True)
        self._dirty_pages.add(key)
        self._schedule_page_refresh(key)

    def _date_range(self) -> tuple[str, str]:
        try:
            start = date.fromisoformat(self.date_from_var.get().strip())
            end = date.fromisoformat(self.date_to_var.get().strip())
        except ValueError as error:
            raise ValueError("日期格式应为 YYYY-MM-DD。") from error
        if start > end:
            raise ValueError("开始日期不能晚于结束日期。")
        return start.isoformat(), end.isoformat()

    def _apply_date_preset(self, _event: tk.Event[Any] | None = None) -> None:
        preset = self.date_preset_var.get()
        if preset == "自定义":
            return
        today = date.today()
        if preset == "近7天":
            start, end = today - timedelta(days=6), today
        elif preset == "本月":
            start, end = today.replace(day=1), today
        elif preset == "上月":
            end = today.replace(day=1) - timedelta(days=1)
            start = end.replace(day=1)
        else:
            start, end = today - timedelta(days=29), today
        self.date_from_var.set(start.isoformat())
        self.date_to_var.set(end.isoformat())
        self.refresh_all()

    def refresh_all(self) -> None:
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        self._dirty_pages.update(self.page_frames)
        self.status_var.set(f"已切换日期：{date_from} 至 {date_to}；正在刷新当前页面…")
        self._schedule_page_refresh(self.current_page)

    def _refresh_dashboard(self) -> None:
        self.metric_labels["revenue"].configure(text=self._fmt_amount(self.summary["revenue"]))
        self.metric_labels["orders"].configure(text=_fmt_int(self.summary["orders"]))
        self.metric_labels["spend"].configure(text=self._fmt_amount(self.summary["spend"]))
        self.metric_labels["roas"].configure(text=f"{self.summary['roas']:.2f}")
        for key, label in self.health_labels.items():
            value = self.summary.get(key, 0)
            available_key = {
                "conversion": "traffic_available",
                "returns": "returns_available",
                "delivered": "delivered_available",
                "cancellations": "cancellations_available",
                "cancellation_rate": "cancellations_available",
            }.get(key)
            if available_key and not self.summary.get(available_key, False):
                text = "—"
            elif key in {"acos", "tacos", "drr", "ctr", "conversion", "cancellation_rate"}:
                text = f"{float(value or 0):.2f}%"
            else:
                text = _fmt_int(value)
                if key == "returns" and not self.summary.get("returns_exact", True):
                    text = "≈" + text
            label.configure(text=text)
        self._draw_trend()

    def _set_trend_mode(self, mode: str, redraw: bool = True) -> None:
        self.trend_mode_var.set(mode)
        for key, button in self.trend_mode_buttons.items():
            selected = key == mode
            button.configure(
                bg="#FFFFFF" if selected else "#F1F5FA",
                fg=PALETTE.primary if selected else PALETTE.muted,
                activebackground="#FFFFFF" if selected else "#E7EDF6",
                activeforeground=PALETTE.primary if selected else PALETTE.text,
            )
        if mode == "volume":
            self.trend_title_label.configure(text="销量与广告订单趋势")
            self.trend_subtitle_label.configure(text="按天汇总，单位：件 / 单")
        else:
            self.trend_title_label.configure(text="销售额与广告花费趋势")
            self.trend_subtitle_label.configure(text=f"按天汇总，单位：{self._currency_code()}")
        if redraw:
            self._draw_trend()

    def _draw_trend(self) -> None:
        if not hasattr(self, "trend_canvas"):
            return
        canvas = self.trend_canvas
        canvas.delete("all")
        self._trend_hover_points: list[dict[str, Any]] = []
        width = max(canvas.winfo_width(), 480)
        height = max(canvas.winfo_height(), 240)
        pad_left, pad_right, pad_top, pad_bottom = 55, 18, 20, 34
        rows = getattr(self, "trend_rows", [])
        if not rows:
            canvas.create_text(width / 2, height / 2, text="暂无数据，请同步 API 或生成演示数据",
                               fill=PALETTE.muted, font=("Microsoft YaHei UI", 10))
            return
        mode = self.trend_mode_var.get() if hasattr(self, "trend_mode_var") else "revenue"
        if mode == "volume":
            primary_key, secondary_key = "orders", "ad_orders"
            primary_label, secondary_label = "销量", "广告订单"
        else:
            primary_key, secondary_key = "revenue", "spend"
            primary_label, secondary_label = "销售额", "广告花费"
        max_value = max(
            max(float(row.get(primary_key, 0) or 0), float(row.get(secondary_key, 0) or 0))
            for row in rows
        ) or 1
        chart_width = width - pad_left - pad_right
        chart_height = height - pad_top - pad_bottom
        for index in range(5):
            y = pad_top + chart_height * index / 4
            canvas.create_line(pad_left, y, width - pad_right, y, fill="#E9EEF5")
            label_value = max_value * (1 - index / 4)
            canvas.create_text(pad_left - 8, y, text=_short_number(label_value), anchor="e",
                               fill="#98A2B3", font=("Segoe UI", 8))
        step = chart_width / max(len(rows) - 1, 1)
        primary_points: list[float] = []
        secondary_points: list[float] = []
        for index, row in enumerate(rows):
            x = pad_left + index * step
            primary = float(row.get(primary_key, 0) or 0)
            secondary = float(row.get(secondary_key, 0) or 0)
            primary_y = pad_top + chart_height * (1 - primary / max_value)
            secondary_y = pad_top + chart_height * (1 - secondary / max_value)
            primary_points.extend([x, primary_y])
            secondary_points.extend([x, secondary_y])
            self._trend_hover_points.append(
                {
                    "x": x, "primary_y": primary_y, "secondary_y": secondary_y,
                    "day": row.get("day", ""), "primary": primary, "secondary": secondary,
                    "primary_label": primary_label, "secondary_label": secondary_label,
                    "mode": mode, "top": pad_top, "bottom": pad_top + chart_height,
                }
            )
        if len(primary_points) >= 4:
            canvas.create_line(*primary_points, fill=PALETTE.primary, width=2.5, smooth=True)
            canvas.create_line(*secondary_points, fill=PALETTE.amber, width=2.2, smooth=True)
        tick_indexes = sorted({0, len(rows) // 2, len(rows) - 1})
        for index in tick_indexes:
            x = pad_left + index * step
            canvas.create_text(x, height - 15, text=str(rows[index]["day"])[5:], fill="#98A2B3",
                               font=("Segoe UI", 8))
        canvas.create_rectangle(width - 190, 10, width - 180, 20, fill=PALETTE.primary, outline="")
        canvas.create_text(width - 174, 15, text=primary_label, anchor="w", fill=PALETTE.muted,
                           font=("Microsoft YaHei UI", 8))
        canvas.create_rectangle(width - 105, 10, width - 95, 20, fill=PALETTE.amber, outline="")
        canvas.create_text(width - 89, 15, text=secondary_label, anchor="w", fill=PALETTE.muted,
                           font=("Microsoft YaHei UI", 8))

    def _trend_hover(self, event: tk.Event[Any]) -> None:
        points = getattr(self, "_trend_hover_points", [])
        canvas = getattr(self, "trend_canvas", None)
        if not points or canvas is None:
            return
        point = min(points, key=lambda item: abs(item["x"] - event.x))
        if abs(point["x"] - event.x) > 22:
            canvas.delete("trend_hover")
            return
        canvas.delete("trend_hover")
        x = point["x"]
        canvas.create_line(
            x, point["top"], x, point["bottom"], fill="#98A2B3", dash=(3, 3),
            tags="trend_hover",
        )
        canvas.create_oval(
            x - 4, point["primary_y"] - 4, x + 4, point["primary_y"] + 4,
            fill=PALETTE.primary, outline="#FFFFFF", width=1, tags="trend_hover",
        )
        canvas.create_oval(
            x - 4, point["secondary_y"] - 4, x + 4, point["secondary_y"] + 4,
            fill=PALETTE.amber, outline="#FFFFFF", width=1, tags="trend_hover",
        )
        formatter = self._fmt_amount if point["mode"] == "revenue" else _fmt_int
        text = (
            f"{point['day']}\n"
            f"{point['primary_label']}：{formatter(point['primary'])}\n"
            f"{point['secondary_label']}：{formatter(point['secondary'])}"
        )
        anchor = "nw" if x < canvas.winfo_width() - 215 else "ne"
        text_x = x + 10 if anchor == "nw" else x - 10
        text_id = canvas.create_text(
            text_x, point["top"] + 24, text=text, anchor=anchor,
            fill=PALETTE.text, font=("Microsoft YaHei UI", 9),
            tags="trend_hover", justify="left",
        )
        bbox = canvas.bbox(text_id)
        if bbox:
            rect = canvas.create_rectangle(
                bbox[0] - 9, bbox[1] - 7, bbox[2] + 9, bbox[3] + 7,
                fill="#FFFFFF", outline=PALETTE.border, tags="trend_hover",
            )
            canvas.tag_lower(rect, text_id)

    def refresh_advertising(self) -> None:
        if not hasattr(self, "ad_tree"):
            return
        date_from, date_to = self._date_range()
        filters = getattr(self, "ad_filter_vars", {})
        try:
            min_spend = _optional_nonnegative_float(filters["min_spend"].get(), "最低花费") if filters else None
            max_acos = _optional_nonnegative_float(filters["max_acos"].get(), "最高 ACOS") if filters else None
        except ValueError as error:
            messagebox.showwarning("筛选条件错误", str(error), parent=self)
            return
        state = filters["state"].get().strip() if filters else ""
        payment_type = filters["payment_type"].get().strip() if filters else ""
        rows = self.database.top_campaigns(
            date_from, date_to,
            query=filters["query"].get().strip() if filters else "",
            state="" if state == "全部" else state,
            payment_type="" if payment_type == "全部" else payment_type,
            min_spend=min_spend,
            max_acos=max_acos,
        )
        summary = self.database.advertising_summary(date_from, date_to)
        self._clear_tree(self.ad_tree)
        for index, row in enumerate(rows):
            self.ad_tree.insert("", "end", values=(
                row["campaign_name"] or row["campaign_id"], _fmt_int(row["impressions"]),
                _fmt_int(row["clicks"]), f"{float(row['ctr']):.2f}%",
                _fmt_int(row["cart_adds"]), f"{float(row['conversion']):.2f}%",
                _fmt_int(row["orders"]), self._fmt_amount(row["spend"]),
                self._fmt_amount_decimal(row["cpc"]), self._fmt_amount_decimal(row["cpa"]),
                self._fmt_amount(row["revenue"]), f"{float(row['roas']):.2f}",
                f"{float(row['acos']):.2f}%",
            ), tags=("odd",) if index % 2 else ())
        if hasattr(self, "ad_filter_count_label"):
            self.ad_filter_count_label.configure(text=f"{len(rows)} 条计划")
        self._refresh_campaign_filter_options(date_from, date_to)
        if hasattr(self, "ad_summary_labels"):
            self.ad_summary_labels["spend"].configure(text=self._fmt_amount(summary["spend"]))
            self.ad_summary_labels["ad_revenue"].configure(text=self._fmt_amount(summary["revenue"]))
            self.ad_summary_labels["ad_orders"].configure(text=_fmt_int(summary["orders"]))
            self.ad_summary_labels["ctr"].configure(text=f"{float(summary['ctr']):.2f}")
            self.ad_summary_labels["cpc"].configure(text=self._fmt_amount_decimal(summary["cpc"]))
            self.ad_summary_labels["roas"].configure(text=f"{float(summary['roas']):.2f}")
        if hasattr(self, "ad_funnel_labels"):
            for key in ("impressions", "clicks", "cart_adds", "orders"):
                self.ad_funnel_labels[key].configure(text=_fmt_int(summary[key]))
            self.ad_funnel_note.configure(
                text=(
                    f"点击→加购 {float(summary['cart_rate']):.2f}%  ·  "
                    f"点击→下单 {float(summary['conversion']):.2f}%  ·  "
                    f"每单广告费 {self._fmt_amount_decimal(summary['cpa'], include_currency=True)}"
                )
            )
        if hasattr(self, "ad_scope_label"):
            if int(summary.get("row_count") or 0):
                self.ad_scope_label.configure(
                    text=(
                        f"当前范围共有 {int(summary['row_count'])} 条活动日报，实际缓存覆盖 "
                        f"{summary.get('available_from') or '—'} 至 {summary.get('available_to') or '—'}。\n\n"
                        f"本期广告消耗 {self._fmt_amount(summary['spend'], include_currency=True)}，"
                        f"归因销售额 {self._fmt_amount(summary['revenue'], include_currency=True)}，"
                        f"付费 ROAS {float(summary['roas']):.2f}，ACOS {float(summary['acos']):.2f}%。\n\n"
                        "数据口径：仅计入活动总计（SKU 为空）的 Performance 日报；SKU 明细用于商品利润分析，不与此处重复相加。"
                    )
                )
            else:
                self.ad_scope_label.configure(
                    text="当前日期范围没有已同步的活动日报。请在“数据中心”执行 Performance 广告同步后再查看广告漏斗和计划表现。"
                )


    def _refresh_campaign_filter_options(
        self, date_from: str | None = None, date_to: str | None = None,
    ) -> None:
        if not hasattr(self, "ad_state_combo"):
            return
        if date_from is None or date_to is None:
            try:
                date_from, date_to = self._date_range()
            except ValueError:
                date_from = date_to = None
        options = self.database.campaign_filter_options(date_from, date_to)
        self._ad_state_values = ("全部", *options.get("states", []))
        self._ad_payment_values = ("全部", *options.get("payment_types", []))
        self.ad_state_combo.configure(values=self._ad_state_values)
        self.ad_payment_combo.configure(values=self._ad_payment_values)
        if self.ad_filter_vars["state"].get() not in self.ad_state_combo.cget("values"):
            self.ad_filter_vars["state"].set("全部")
        if self.ad_filter_vars["payment_type"].get() not in self.ad_payment_combo.cget("values"):
            self.ad_filter_vars["payment_type"].set("全部")

    def reset_ad_filters(self) -> None:
        if not hasattr(self, "ad_filter_vars"):
            return
        for key, variable in self.ad_filter_vars.items():
            variable.set("全部" if key in {"state", "payment_type"} else "")
        self.refresh_advertising()

    def refresh_sales(self) -> None:
        if not hasattr(self, "sales_tree"):
            return
        date_from, date_to = self._date_range()
        rows = self.database.top_products(date_from, date_to)
        query = self.sales_search_var.get().strip().lower() if hasattr(self, "sales_search_var") else ""
        if query:
            rows = [
                row for row in rows
                if query in str(row["sku"]).lower()
                or query in str(row.get("offer_id", "")).lower()
                or query in str(row["product_name"]).lower()
            ]
        self._clear_tree(self.sales_tree)
        missing: set[str] = set()
        for index, row in enumerate(rows):
            if not row.get("offer_id"):
                missing.add("货号")
            if not row.get("delivered_available"):
                missing.add("妥投")
            if not row.get("returns_available"):
                missing.add("退货")
            if not row.get("cancellations_available"):
                missing.add("取消")
            if not row.get("traffic_available"):
                missing.add("浏览/加购")
            self.sales_tree.insert("", "end", values=(
                row.get("offer_id") or "—", row["sku"], row["product_name"],
                _fmt_int(row["orders"]),
                _fmt_int(row["delivered"]) if row.get("delivered_available") else "—",
                (
                    ("" if row.get("returns_exact", True) else "≈")
                    + _fmt_int(row["returns"])
                ) if row.get("returns_available") else "—",
                _fmt_int(row["cancellations"]) if row.get("cancellations_available") else "—",
                f"{float(row['cancellation_rate'] or 0):.2f}%"
                if row.get("cancellations_available") and row.get("cancellation_rate") is not None else "—",
                _fmt_int(row["views"]) if row.get("traffic_available") else "—",
                _fmt_int(row["cart_adds"]) if row.get("traffic_available") else "—",
                f"{float(row['conversion'] or 0):.2f}%"
                if row.get("traffic_available") and row.get("conversion") is not None else "—",
                _fmt_money(row["revenue"]),
            ), tags=("odd",) if index % 2 else ())
        if hasattr(self, "sales_data_hint"):
            if missing:
                traffic_status = self.database.get_setting("seller_traffic_status", "unknown")
                if "浏览/加购" in missing and traffic_status == "subscription_required":
                    traffic_note = (
                        "Ozon Seller /v1/analytics/data 本次只返回基础销量指标；"
                        "后台虽有浏览数据，但当前 Analytics 订阅/API Key 未向接口返回该字段。"
                    )
                elif "浏览/加购" in missing:
                    traffic_note = "下次 Seller 同步会在完整日志中记录接口实际返回的指标数量。"
                else:
                    traffic_note = ""
                self.sales_data_hint.configure(
                    text="暂无数据字段：" + "、".join(sorted(missing))
                    + "。无权限字段不再显示为 0。" + traffic_note
                    + "“≈退货”按财务确认记录的原始下单日期估算；"
                    "当前 API 权限未返回后台原生退货指标。"
                )
            else:
                self.sales_data_hint.configure(
                    text="“≈退货”按财务确认记录的原始下单日期估算；"
                    "当前 API 权限未返回后台原生退货指标；"
                    "取消率 = 取消件数 ÷ 下单件数。"
                )

    def refresh_product_profit(self) -> None:
        if not hasattr(self, "product_profit_tree"):
            return
        if self.active_shop.kind == "cross_border":
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        query = self.product_profit_search_var.get().strip()
        rows = self.database.product_unit_profit_rows(
            date_from, date_to, query=query, limit=600,
        )
        self._clear_tree(self.product_profit_tree)
        self._product_profit_rows_by_iid = {}
        for index, row in enumerate(rows):
            sku = str(row.get("sku") or "")
            if not sku:
                continue
            iid = sku
            self._product_profit_rows_by_iid[iid] = row
            self.product_profit_tree.insert("", "end", iid=iid, values=(
                row.get("offer_id") or "—", sku,
                _fmt_int(row.get("reference_units")),
                _fmt_money(row.get("selling_price_unit")),
                _fmt_money(row.get("ad_spend_unit")),
                f"{float(row['tacos']):.2f}%" if row.get("tacos") is not None else "—",
                _fmt_money(row.get("delivery_unit")) if row.get("delivery_unit") is not None else "—",
                row.get("delivery_source") or "—",
                _fmt_money(row.get("commission_unit")),
                _fmt_money(row.get("acquiring_unit")),
                _fmt_money(row.get("unit_cost")) if row.get("unit_cost") is not None else "—",
                _fmt_money(row.get("first_mile_cost")) if row.get("first_mile_cost") is not None else "—",
                _fmt_money(row.get("tax_unit")),
                _fmt_money(row.get("payout_fee_unit")),
                _fmt_money(row.get("pre_tax_profit_unit"))
                if row.get("pre_tax_profit_unit") is not None else "—",
                _fmt_money(row.get("after_tax_profit_unit"))
                if row.get("after_tax_profit_unit") is not None else "—",
            ), tags=("odd",) if index % 2 else ())
        complete = [row for row in rows if row.get("pre_tax_profit_unit") is not None]
        revenue = _average(rows, "selling_price_unit")
        ad_spend = _average(rows, "ad_spend_unit")
        pre_tax = _average(complete, "pre_tax_profit_unit")
        after_tax = _average(complete, "after_tax_profit_unit")
        self.product_profit_summary_labels["products"].configure(
            text=_fmt_int(len(rows))
        )
        self.product_profit_summary_labels["revenue"].configure(text=_fmt_money(revenue))
        self.product_profit_summary_labels["ad_spend"].configure(text=_fmt_money(ad_spend))
        self.product_profit_summary_labels["pre_tax"].configure(
            text=_fmt_money(pre_tax) if complete else "—"
        )
        self.product_profit_summary_labels["after_tax"].configure(
            text=_fmt_money(after_tax) if complete else "—"
        )
        cache = self.database.ad_detail_cache_info(date_from, date_to)
        route_count = sum(1 for row in rows if row.get("delivery_source") == "集群配送表")
        missing = len(rows) - len(complete)
        cache_text = (
            f"SKU 广告缓存 {cache['row_count']} 条，更新于 {cache['updated_at']}"
            if cache["complete"] else "当前日期尚未同步 SKU 广告明细"
        )
        self.product_profit_hint.configure(
            text=(
                f"显示 {len(rows)} 个单品预测 · {cache_text} · {route_count} 个商品采用集群配送表 · "
                f"{missing} 个商品缺少成本或配送依据。表内金额均为每卖 1 件的预测值。"
            )
        )

    def import_delivery_tariff_workbook(self) -> None:
        initial = self.database.get_setting("delivery_tariff_source", "")
        path = filedialog.askopenfilename(
            parent=self, title="选择 Ozon 集群配送费表",
            initialdir=str(Path(initial).parent) if initial else None,
            filetypes=[("Excel 工作簿", "*.xlsx"), ("所有文件", "*.*")],
        )
        if not path:
            return
        try:
            result = self.database.import_delivery_tariffs(path)
        except Exception as error:
            messagebox.showerror("导入失败", str(error), parent=self)
            return
        self.refresh_orders()
        self.status_var.set(
            f"已导入 {result['row_count']} 条配送费规则，覆盖 {result['routes']} 条集群路线。"
        )

    def refresh_orders(self) -> None:
        if not hasattr(self, "order_detail_tree"):
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            self.status_var.set(str(error))
            return
        rows = self.database.order_detail_rows(
            date_from, date_to, query=self.order_search_var.get().strip(),
            scheme=self.order_scheme_var.get(), limit=5000,
        )
        self._clear_tree(self.order_detail_tree)
        for index, row in enumerate(rows):
            self.order_detail_tree.insert("", "end", values=(
                row.get("day") or "—", row.get("posting_number") or "—",
                row.get("status") or "—", row.get("scheme") or "—",
                row.get("offer_id") or "—", row.get("sku") or "—",
                _fmt_int(row.get("quantity")), self._fmt_amount(row.get("order_price")),
                row.get("origin_normalized") or row.get("origin") or "—",
                row.get("destination_normalized") or row.get("destination") or "—",
                f"{float(row['volume_l']):.3f}" if row.get("volume_l") is not None else "—",
                f"{float(row['weight_kg']):.3f}" if row.get("weight_kg") is not None else "—",
                self._fmt_amount(row.get("estimated_delivery_unit"))
                if row.get("estimated_delivery_unit") is not None else "—",
                self._fmt_amount(row.get("estimated_delivery_total"))
                if row.get("estimated_delivery_total") is not None else "—",
                row.get("tariff_status") or "—",
            ), tags=("odd",) if index % 2 else ())
        info = self.database.delivery_tariff_info()
        self.order_tariff_hint.configure(
            text=(
                f"配送费缓存 {int(info.get('row_count') or 0)} 条 / "
                f"{int(info.get('routes') or 0)} 条路线 · {info.get('source_file') or '尚未导入'}"
            )
        )
        matched = sum(1 for row in rows if row.get("tariff_status") == "已匹配")
        self.status_var.set(f"已显示 {len(rows)} 条订单商品行，其中 {matched} 条已估算基础配送费。")

    def refresh_product_daily(self) -> None:
        if not hasattr(self, "product_daily_tree") or self.active_shop.kind == "cross_border":
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            self.status_var.set(str(error))
            return
        rows = self.database.daily_product_profit_rows(
            date_from, date_to, query=self.product_daily_search_var.get().strip(), limit=10000,
        )
        self._clear_tree(self.product_daily_tree)
        source_labels = {
            "official_sku": "官方 SKU",
            "unique_campaign_title": "专属活动",
            "mixed": "混合口径",
        }
        for index, row in enumerate(rows):
            self.product_daily_tree.insert("", "end", values=(
                row.get("day") or "—", row.get("offer_id") or "—", row.get("sku") or "—",
                _fmt_int(row.get("units")), _fmt_money(row.get("revenue")),
                _fmt_money(row.get("ad_spend")), _fmt_int(row.get("ad_orders")),
                source_labels.get(str(row.get("ad_attribution_source") or ""), "无匹配"),
                _fmt_money(row.get("ad_cost_per_order"))
                if row.get("ad_cost_per_order") is not None else "—",
                f"{float(row['tacos']):.2f}%" if row.get("tacos") is not None else "—",
                _fmt_money(row.get("estimated_profit"))
                if row.get("estimated_profit") is not None else "—",
                f"{float(row['profit_rate']):.2f}%" if row.get("profit_rate") is not None else "—",
                "完整" if row.get("cost_profile_complete") else "缺成本/配送依据",
            ), tags=("odd",) if index % 2 else ())
        incomplete = sum(1 for row in rows if not row.get("cost_profile_complete"))
        official = sum(
            1 for row in rows
            if row.get("ad_attribution_source") in {"official_sku", "mixed"}
        )
        mapped = sum(
            1 for row in rows
            if row.get("ad_attribution_source") in {"unique_campaign_title", "mixed"}
        )
        self.product_daily_hint.configure(
            text=(
                f"共 {len(rows)} 条 SKU-日期记录；广告匹配：官方 SKU {official} 条、"
                f"专属活动 {mapped} 条；{incomplete} 条缺少成本或配送依据。"
                "每日利润为经营估算，月度应计盈亏仍以 Ozon Finance 为最终口径。"
            )
        )

    def refresh_product_series(self) -> None:
        if not hasattr(self, "product_series_tree"):
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            self.status_var.set(str(error))
            return
        label = self.product_series_period_var.get()
        period = {"每天": "day", "每7天": "week", "每月": "month"}.get(label, "week")
        rows = self.database.product_series_rows(
            date_from, date_to, period=period,
            query=self.product_series_search_var.get().strip(),
        )
        self._product_series_rows = rows
        self._clear_tree(self.product_series_tree)
        for index, row in enumerate(rows):
            self.product_series_tree.insert("", "end", values=(
                row.get("period") or "—", row.get("series") or "—",
                _fmt_int(row.get("sku_count") or 0), _fmt_int(row.get("units") or 0),
                _fmt_money(row.get("revenue") or 0),
            ), tags=("odd",) if index % 2 else ())
        self.product_series_hint.configure(
            text=(
                f"当前按“{label}”汇总 {len(rows)} 条系列周期记录。"
                "发送飞书群只由右上按钮手动触发，不会自动发送。"
            )
        )

    def send_product_series_to_feishu_group(self) -> None:
        rows = list(getattr(self, "_product_series_rows", []))
        if not rows:
            messagebox.showwarning("没有可发送数据", "请先刷新产品系列统计。", parent=self)
            return
        if not messagebox.askyesno(
            "确认发送飞书群",
            f"将当前显示的 {len(rows)} 条产品系列统计发送到当前店铺配置的飞书群？\n\n"
            "这是手动操作，不会写入多维表格，也不会自动定时发送。",
            parent=self,
        ):
            return
        period_type = self.product_series_period_var.get()
        credentials = self._credentials_from_form()

        def action() -> dict[str, Any]:
            self.service.save_credentials(credentials)
            return self.service.send_feishu_product_series(
                rows, period_type, credentials,
            )

        def finished(result: dict[str, Any]) -> None:
            message = (
                f"产品系列分析已发送到飞书群："
                f"{result['rows']} 条统计，共 {result['messages']} 张卡片。"
            )
            self.status_var.set(message)
            messagebox.showinfo("发送完成", message, parent=self)

        self._run_task("正在发送产品系列分析到飞书群…", action, finished)

    def send_product_series_to_feishu(self) -> None:
        """保留旧版回调名；升级后同样只发送群消息。"""
        self.send_product_series_to_feishu_group()

    def refresh_shipment_tracking(self) -> None:
        if not hasattr(self, "shipment_tracking_tree"):
            return
        rows = self.database.shipment_tracking_rows(
            self.shipment_tracking_search_var.get().strip()
        )
        self._clear_tree(self.shipment_tracking_tree)
        arrived = pending = 0
        for index, row in enumerate(rows):
            foreign_arrival = str(row.get("foreign_arrival") or "")
            notified = str(row.get("notified_foreign_arrival") or "")
            if foreign_arrival:
                arrived += 1
                notification = "已发送" if notified == foreign_arrival else "待发送"
                if notification == "待发送":
                    pending += 1
            else:
                notification = "未到库"
            self.shipment_tracking_tree.insert("", "end", iid=str(row.get("tracking_id") or index), values=(
                row.get("tracking_id") or "—", row.get("product_name") or "—",
                row.get("batch_no") or "—", row.get("shop_name") or "—",
                _fmt_int(row.get("quantity") or 0), row.get("cargo_status") or "—",
                row.get("channel") or "—", row.get("domestic_arrival") or "—",
                foreign_arrival or "—", notification,
                str(row.get("updated_at") or "").replace("T", " "),
            ), tags=("odd",) if index % 2 else ())
        summaries = {
            "total": len(rows), "in_transit": len(rows) - arrived,
            "arrived": arrived, "pending": pending,
        }
        for key, value in summaries.items():
            self.shipment_tracking_summary_labels[key].configure(text=_fmt_int(value))
        self.shipment_tracking_hint.configure(
            text=(
                f"本地缓存 {len(rows)} 条；国外到库 {arrived} 条，待发送 {pending} 条。"
                "双击一条已到库记录可预览并确认发送飞书；自动检测不会自动发群。"
            )
        )

    def start_shipment_tracking_sync(self, *, automatic: bool = False) -> None:
        credentials = self.service.load_credentials() if automatic else self._credentials_from_form()
        if not automatic:
            try:
                self.service.save_credentials(credentials)
            except Exception as error:
                messagebox.showerror("保存失败", str(error), parent=self)
                return
        event_listener = lambda *event: self.after(0, self._on_log_event, *event)

        def finished(result: dict[str, Any]) -> None:
            self.refresh_shipment_tracking()
            self.refresh_logs(preferred_log_id=result.get("log_id"))
            self.database.save_settings({
                "shipment_tracking_last_success_at": datetime.now().isoformat(timespec="seconds")
            })
            message = (
                f"发货跟踪已读取 {result['rows']} 条，检测到 {result['arrivals']} 条到库变化，"
                "通知均保留为待人工确认。"
            )
            self.status_var.set(message)
            if not automatic:
                messagebox.showinfo("发货跟踪更新完成", message, parent=self)

        self._run_task(
            "正在读取飞书发货跟踪并检测国外到库…",
            lambda: self.service.sync_feishu_shipments(
                credentials, event_listener=event_listener,
            ),
            finished,
        )

    def send_selected_shipment_to_feishu(
        self, event: tk.Event[Any] | None = None,
    ) -> None:
        if event is not None and getattr(event, "num", None) == 1:
            iid = self.shipment_tracking_tree.identify_row(event.y)
            if iid:
                self.shipment_tracking_tree.selection_set(iid)
        selected = self.shipment_tracking_tree.selection()
        if not selected:
            return
        tracking_id = str(selected[0])
        row = self.database.shipment_tracking_row(tracking_id)
        if not row:
            messagebox.showwarning("记录不存在", "本地缓存中没有找到该发货记录。", parent=self)
            return
        arrival = str(row.get("foreign_arrival") or "").strip()
        if not arrival:
            messagebox.showinfo("尚未到库", "该记录尚未填写国外到库日期，暂不能发送。", parent=self)
            return
        already_sent = str(row.get("notified_foreign_arrival") or "") == arrival
        prompt = (
            f"采购/跟踪单号：{tracking_id}\n"
            f"品名：{row.get('product_name') or '—'}\n"
            f"店铺：{row.get('shop_name') or self.active_shop.display_name}\n"
            f"数量：{_fmt_int(row.get('quantity') or 0)}\n"
            f"国外到库：{arrival}\n\n"
        )
        if already_sent:
            prompt += "该到库日期已经发送过，是否仍要再次发送？"
        else:
            prompt += "是否将这条到库信息发送到飞书群？"
        if not messagebox.askyesno("发送到库通知", prompt, parent=self):
            return

        def finished(_message_id: str) -> None:
            self.refresh_shipment_tracking()
            self.status_var.set(f"到库通知已发送：{tracking_id}")
            messagebox.showinfo("发送成功", "该到库信息已经发送到飞书群。", parent=self)

        self._run_task(
            "正在发送选中的到库信息到飞书群…",
            lambda: self.service.send_feishu_shipment_notification(tracking_id),
            finished,
        )

    def sync_product_profit_ads(self) -> None:
        selection = self.product_profit_tree.selection()
        if not selection:
            messagebox.showinfo(
                "请选择商品", "请先在产品利润表中选择一个 SKU；一次报告会缓存该日期范围内的全部 SKU。",
                parent=self,
            )
            return
        row = self._product_profit_rows_by_iid.get(selection[0])
        if not row:
            return
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        sku = str(row.get("sku") or "")

        def action() -> Any:
            return self.service.sync_performance_product_detail(
                date_from, date_to, sku, cancel_event=self._cancel_event,
            )

        def done(result: Any) -> None:
            self.refresh_product_profit()
            self.refresh_logs()
            self.status_var.set(str(getattr(result, "message", "商品广告缓存已更新")))

        self._run_task("正在读取或复用本地 SKU 广告明细缓存…", action, done)

    def open_product_profit_detail(self, _event: tk.Event[Any] | None = None) -> None:
        selection = self.product_profit_tree.selection()
        if not selection:
            return
        row = self._product_profit_rows_by_iid.get(selection[0])
        if not row:
            return
        date_from, date_to = self._date_range()
        sku = str(row.get("sku") or "")
        daily_rows = self.database.product_ad_daily_rows(date_from, date_to, sku)
        window = tk.Toplevel(self)
        window.title(f"产品利润与广告明细 · {row.get('offer_id') or sku}")
        window.geometry("1280x760")
        window.minsize(980, 620)
        window.configure(bg=PALETTE.bg)
        window.transient(self)
        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=22, pady=(17, 10))
        tk.Label(
            header, text=f"{row.get('offer_id') or '无货号'} / {sku}",
            bg=PALETTE.bg, fg=PALETTE.text,
            font=("Microsoft YaHei UI", 17, "bold"),
        ).pack(side="left")
        self._button(
            header, "填写采购成本", lambda: self._show_product_cost_editor(row, cross_border=False),
            secondary=True, compact=True,
        ).pack(side="right")
        formula = self._panel(window)
        formula.pack(fill="x", padx=22, pady=(0, 10))
        tk.Label(
            formula,
            text=(
                "单件税前利润 = 单件售价 − 单件采购成本 − 单件头程(RUB) − 基础配送/件"
                " − 单件佣金 − 单件收单费 − 单件广告费\n"
                f"单件税后利润 = 单件税前利润 − 单件售价×{float(row.get('tax_rate') or 0):.2f}%"
                f" − 预计回款×{float(row.get('payout_fee_rate') or 0):.2f}% ；"
                "月度应计费用表仍是最终结算依据。"
            ),
            justify="left", anchor="w", wraplength=1180,
            bg="#FFFFFF", fg=PALETTE.text, font=("Microsoft YaHei UI", 9),
        ).pack(fill="x", padx=16, pady=11)
        route = tk.Label(
            window,
            text=(
                f"单件售价 {_fmt_money(row.get('selling_price_unit'))} ₽  ·  "
                f"基础配送 {_fmt_money(row.get('delivery_unit')) if row.get('delivery_unit') is not None else '—'} ₽/件"
                f"（{row.get('delivery_source') or '无依据'}，匹配 {int(row.get('route_match_units') or 0)} 件订单） ·  "
                f"单件税后利润 {_fmt_money(row.get('after_tax_profit_unit')) if row.get('after_tax_profit_unit') is not None else '—'} ₽"
            ),
            anchor="w", bg=PALETTE.bg, fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        )
        route.pack(fill="x", padx=24, pady=(0, 5))
        panel = self._panel(window)
        panel.pack(fill="both", expand=True, padx=22, pady=(0, 18))
        tk.Label(
            panel,
            text=("逐日广告效率（仅精确 SKU 缓存；‘广告费用占比’="
                  "SKU 广告费÷店铺广告费；‘广告销售占比’=广告销售额÷Seller 销售额）"),
            anchor="w", bg="#FFFFFF", fg=PALETTE.primary,
            font=("Microsoft YaHei UI", 9, "bold"),
        ).pack(fill="x", padx=16, pady=(12, 2))
        tree = self._tree(panel, [
            ("day", "日期", 105, "w"), ("sales", "Seller 销售额", 115, "e"),
            ("units", "销量", 75, "e"), ("spend", "广告费", 100, "e"),
            ("ad_orders", "广告订单", 85, "e"),
            ("per_order", "每广告订单费用", 120, "e"),
            ("spend_share", "广告费用占比", 105, "e"),
            ("ad_revenue", "广告销售额", 105, "e"),
            ("share", "广告销售占比", 105, "e"),
            ("acos", "ACOS", 75, "e"), ("tacos", "TACOS", 75, "e"),
            ("impressions", "曝光", 90, "e"), ("clicks", "点击", 80, "e"),
            ("ctr", "CTR", 70, "e"),
        ])
        for index, item in enumerate(daily_rows):
            tree.insert("", "end", values=(
                item["day"], _fmt_money(item["seller_revenue"]),
                _fmt_int(item["ordered_units"]), _fmt_money(item["ad_spend"]),
                _fmt_int(item["ad_orders"]),
                _fmt_money(item["ad_cost_per_order"])
                if item["ad_cost_per_order"] is not None else "—",
                f"{float(item['ad_spend_share']):.2f}%"
                if item["ad_spend_share"] is not None else "—",
                _fmt_money(item["ad_revenue"]),
                f"{float(item['ad_sales_share']):.2f}%"
                if item["ad_sales_share"] is not None else "—",
                f"{float(item['acos']):.2f}%" if item["acos"] is not None else "—",
                f"{float(item['tacos']):.2f}%" if item["tacos"] is not None else "—",
                _fmt_int(item["impressions"]), _fmt_int(item["clicks"]),
                f"{float(item['ctr']):.2f}%" if item["ctr"] is not None else "—",
            ), tags=("odd",) if index % 2 else ())

    def refresh_profit(self) -> None:
        if not hasattr(self, "profit_tree"):
            return
        try:
            month_start, month_end = _month_bounds(self.profit_month_var.get())
        except ValueError as error:
            messagebox.showwarning("月份错误", str(error), parent=self)
            return
        date_from, date_to = month_start.isoformat(), month_end.isoformat()
        rows = self.database.monthly_profit_rows(date_from, date_to)
        summary = self.database.monthly_profit_summary(date_from, date_to)
        self._profit_summary = dict(summary)
        self.profit_summary_labels["revenue"].configure(text=_fmt_money(summary["revenue"]))
        self.profit_summary_labels["orders"].configure(text=_fmt_int(summary["orders"]))
        self.profit_summary_labels["sales_returns"].configure(
            text=_fmt_money(summary["sales_returns"]) if summary["finance_available"] else "—"
        )
        self.profit_summary_labels["accrual_fees"].configure(
            text=_fmt_money(summary["accrual_fees"]) if summary["finance_available"] else "—"
        )
        self.profit_summary_labels["other_adjustments"].configure(
            text=_fmt_money(summary["other_adjustments"])
            if summary["finance_available"] else "—"
        )
        self.profit_summary_labels["platform_payout"].configure(
            text=_fmt_money(summary["platform_payout"]) if summary["finance_available"] else "—"
        )
        self.profit_summary_labels["product_cost"].configure(
            text=_fmt_money(summary["product_cost"])
            if summary["configured_product_cost_skus"] else "—"
        )
        self.profit_summary_labels["profit"].configure(
            text=_fmt_money(summary["profit"]) if summary["profit"] is not None else "—"
        )
        self.profit_summary_labels["tax_amount"].configure(
            text=_fmt_money(summary["tax_amount"])
        )
        self.profit_summary_labels["payout_fee"].configure(
            text=_fmt_money(summary["payout_fee"])
        )
        self.profit_summary_labels["after_tax_profit"].configure(
            text=_fmt_money(summary["after_tax_profit"])
            if summary["after_tax_profit"] is not None else "—"
        )
        self._clear_tree(self.profit_unallocated_tree)
        self._clear_tree(self.profit_metric_tree)
        self._profit_unallocated_loaded = False
        self._profit_unallocated_date_range = (date_from, date_to)
        self._profit_rows_by_sku = {str(row["sku"]): row for row in rows}
        self._profit_rows = list(rows)
        self._refresh_profit_product_options(rows)
        self._populate_profit_sku_rows(rows)
        if self._profit_active_metric:
            self.filter_profit_metric(self._profit_active_metric)
        else:
            self.profit_metric_filter_var.set(
                "点击上方指标卡，可在下方查看对应的 SKU 或 API 费用字段"
            )
        notes = [
            f"已读取 {len(rows)} 个 SKU、{summary['operation_count']} 条逐笔应计；"
            f"其中 {summary['unallocated_operations']} 条店铺级/多 SKU 记录未强行分摊"
            f"（净额 {_fmt_money(summary['unallocated_amount'])} ₽）。",
            "订单金额与已订购商品来自 Seller Analytics 的下单日期；"
            "销售与退货（应计）及应计净额来自 Finance 的财务发生日期，二者不可直接当作同一口径。",
            "可对账公式：应计净额 = 销售/退货应计 + 应计费用 + 其他收入/调整。",
            "成本核算金额 = 成本核算件数 × 单件成本；"
            "税前利润 = 应计净额 + 成本核算金额 + 头程核算金额；"
            f"税费估算 = 订单金额 × {summary['tax_rate']:.2f}%；"
            f"回款手续费 = 正应计净额 × {summary['payout_fee_rate']:.2f}%；"
            "税后利润 = 税前利润 - 税费 - 回款手续费。",
        ]
        coverage = summary["seller_sales_coverage"]
        if not summary["seller_sales_complete"]:
            cached_from = coverage.get("min_day") or "无数据"
            cached_to = coverage.get("max_day") or "无数据"
            notes.insert(
                0,
                f"⚠ Seller 月销量缓存不完整：当前仅覆盖 {cached_from} 至 {cached_to}，"
                f"本月需要 {coverage['requested_from']} 至 {coverage['effective_to']}；"
                "订单金额与已订购商品当前是部分值，请点击“同步本月销量”。",
            )
        else:
            notes.insert(
                0,
                f"Seller 月销量已完整覆盖 {coverage['requested_from']} 至 "
                f"{coverage['effective_to']}。",
            )
        if summary["shop_economy_available"]:
            difference = summary.get("reconciliation_difference")
            if difference is not None and abs(float(difference)) >= 0.01:
                notes.append(
                    f"Seller cash_flows 原始汇总字段与逐笔应计相差 "
                    f"{_fmt_money(difference)} ₽；已自动排除异常汇总值，采用逐笔应计总额。"
                )
        else:
            notes.append("尚无商店经济汇总缓存，请点击“同步应计费用”。")
        if summary["missing_cost_skus"]:
            notes.append(
                f"还有 {summary['missing_cost_skus']} 个有销量商品未同时填写成本和头程，"
                "因此总利润与对应商品利润显示为“—”。"
            )
        if summary["missing_product_cost_skus"]:
            notes.append(
                f"成本核算金额当前仅覆盖 "
                f"{summary['configured_product_cost_skus']} 个 SKU / {summary['costed_units']} 件；"
                f"还有 {summary['missing_product_cost_skus']} 个有销量 SKU 未填写单件成本。"
            )
        if not summary["finance_available"]:
            notes.append("该月尚无应计费用，请点击“同步应计费用”直接从 Seller API 获取。")
        self.profit_hint.configure(text="  ".join(notes))
        self.profit_unallocated_hint.configure(
            text=(
                f"共 {summary['unallocated_operations']} 条店铺级/多 SKU 应计，"
                f"净额 {_fmt_money(summary['unallocated_amount'])} ₽。"
                "切换到本页签后逐项加载；这些费用计入店铺应计净额，但不会伪造 SKU 分摊。"
            )
        )
        try:
            if self.profit_notebook.index("current") == 1:
                self._load_profit_unallocated_rows()
        except tk.TclError:
            pass

    def save_profit_parameters(self) -> None:
        try:
            values = {
                "local_tax_rate": float(self.profit_tax_rate_var.get().strip()),
                "local_payout_fee_rate": float(
                    self.profit_payout_fee_rate_var.get().strip()
                ),
                "local_default_delivery_fee": float(
                    self.profit_delivery_default_var.get().strip()
                ),
            }
            if any(value < 0 for value in values.values()):
                raise ValueError("税率、手续费和配送费不能小于 0。")
            if values["local_tax_rate"] > 100 or values["local_payout_fee_rate"] > 100:
                raise ValueError("税率和手续费不能超过 100%。")
            self.database.save_settings({key: str(value) for key, value in values.items()})
            self.refresh_profit()
            if hasattr(self, "product_profit_tree"):
                self.refresh_product_profit()
            self.status_var.set("税费、回款手续费和默认配送估算参数已保存")
        except ValueError as error:
            messagebox.showwarning("参数错误", str(error), parent=self)

    def _refresh_profit_product_options(self, rows: list[dict[str, Any]]) -> None:
        if not hasattr(self, "profit_product_combo"):
            return
        previous = self.profit_product_var.get()
        self._profit_products_by_display = {}
        for row in rows:
            sku = str(row.get("sku") or "").strip()
            if not sku:
                continue
            display = " | ".join(filter(None, (
                str(row.get("offer_id") or "无货号").strip(), sku,
            )))
            self._profit_products_by_display[display] = dict(row)
        self._profit_product_values = tuple(sorted(
            self._profit_products_by_display,
            key=str.casefold,
        ))
        self.profit_product_combo.configure(values=self._profit_product_values)
        if previous:
            self.profit_product_var.set(previous)

    def _filter_profit_products(self, event: tk.Event[Any]) -> None:
        self._filter_combo_keyrelease(
            event, self.profit_product_combo,
            getattr(self, "_profit_product_values", ()), self.profit_product_var,
        )
        query = self.profit_product_var.get().strip().casefold()
        matches = (
            [value for value in self._profit_product_values if query in value.casefold()]
            if query else list(self._profit_product_values)
        )
        self.profit_metric_filter_var.set(
            f"商品模糊搜索：匹配 {len(matches)} 项"
            + ("；请选择一项或继续输入" if len(matches) != 1 else "；按回车可填写成本")
        )

    def open_profit_product_from_combo(self) -> None:
        value = self.profit_product_var.get()
        row = self._product_from_input(self._profit_products_by_display, value)
        if not row:
            query = value.strip().casefold()
            matches = [
                display for display in self._profit_product_values
                if query and query in display.casefold()
            ]
            self.profit_metric_filter_var.set(
                f"当前输入匹配 {len(matches)} 个商品；"
                + ("请从展开列表选择具体货号 / SKU" if matches else "没有匹配商品")
            )
            return
        sku = str(row.get("sku") or "")
        if not sku:
            return
        self._populate_profit_sku_rows(self._profit_rows)
        self.profit_notebook.select(0)
        if self.profit_tree.exists(sku):
            self.profit_tree.selection_set(sku)
            self.profit_tree.focus(sku)
            self.profit_tree.see(sku)
            self.open_profit_sku_detail()

    def _populate_profit_sku_rows(self, rows: list[dict[str, Any]]) -> None:
        self._clear_tree(self.profit_tree)
        for index, row in enumerate(rows):
            finance_available = bool(row.get("finance_rows"))
            self.profit_tree.insert("", "end", iid=str(row["sku"]), values=(
                row.get("offer_id") or "—", row["sku"], _fmt_int(row["orders"]),
                _fmt_money(row["revenue"]), _fmt_int(row["cost_units"]),
                _fmt_money(row["product_cost_total"])
                if row["product_cost_total"] is not None else "—",
                _fmt_money(row["first_mile_total"])
                if row["first_mile_total"] is not None else "—",
                _fmt_money(row["sales_returns"]) if finance_available else "—",
                _fmt_money(row["platform_payout"]) if finance_available else "—",
                _fmt_money(row["commission"]) if finance_available else "—",
                _fmt_money(row["ad_spend"]) if finance_available else "—",
                _fmt_money(row["delivery"]) if finance_available else "—",
                _fmt_money(row["last_mile"]) if finance_available else "—",
                _fmt_money(row["average_delivery_cost"])
                if row["average_delivery_cost"] is not None else "—",
                _fmt_money(row["returns_accrual"]) if finance_available else "—",
                _fmt_money(row["return_logistics"]) if finance_available else "—",
                _fmt_money(row["acquiring"]) if finance_available else "—",
                _fmt_money(row["fbo_storage"]) if finance_available else "—",
                _fmt_money(row["penalties_compensations"]) if finance_available else "—",
                _fmt_money(row["other_fees"]) if finance_available else "—",
                _fmt_decimal(row["unit_cost"]) if row["unit_cost"] is not None else "—",
                _fmt_decimal(row["first_mile_cost"]) if row["first_mile_cost"] is not None else "—",
                _fmt_money(row["profit"]) if row["profit"] is not None else "—",
                f"{float(row['profit_rate']):.2f}%" if row["profit_rate"] is not None else "—",
            ), tags=("odd",) if index % 2 else ())

    def filter_profit_metric(self, metric: str) -> None:
        titles = {
            "revenue": "订单金额", "orders": "已订购商品",
            "sales_returns": "销售/退货应计",
            "accrual_fees": "应计费用", "other_adjustments": "其他收入/调整",
            "platform_payout": "应计净额", "product_cost": "成本核算金额",
            "profit": "税前利润", "tax_amount": "税费估算",
            "payout_fee": "回款手续费", "after_tax_profit": "税后利润",
        }
        if metric not in titles:
            return
        self._profit_active_metric = metric
        for key, card in self.profit_metric_cards.items():
            selected = key == metric
            card.configure(
                highlightbackground=(self.profit_metric_card_colors[key]
                                     if selected else PALETTE.border),
                highlightthickness=2 if selected else 1,
            )
        if metric in {"tax_amount", "payout_fee", "after_tax_profit"}:
            self.profit_notebook.select(2)
            self._clear_tree(self.profit_metric_tree)
            formula = {
                "tax_amount": (
                    f"订单金额 × {float(self._profit_summary.get('tax_rate') or 0):.2f}%"
                ),
                "payout_fee": (
                    f"正应计净额 × {float(self._profit_summary.get('payout_fee_rate') or 0):.2f}%"
                ),
                "after_tax_profit": "税前利润 - 税费估算 - 回款手续费",
            }[metric]
            amount = self._profit_summary.get(metric)
            self.profit_metric_tree.insert("", "end", values=(
                titles[metric], "本地估算参数", titles[metric], formula,
                "1", _fmt_money(amount) if amount is not None else "—",
            ))
            self.profit_metric_filter_var.set(
                f"当前筛选：{titles[metric]} · 公式：{formula}"
            )
            return
        if metric in {"revenue", "orders", "product_cost", "profit"}:
            if metric == "revenue":
                rows = [row for row in self._profit_rows if float(row.get("revenue") or 0)]
                rows.sort(key=lambda row: float(row.get("revenue") or 0), reverse=True)
            elif metric == "orders":
                rows = [row for row in self._profit_rows if int(row.get("orders") or 0)]
                rows.sort(key=lambda row: int(row.get("orders") or 0), reverse=True)
            elif metric == "product_cost":
                rows = [
                    row for row in self._profit_rows
                    if row.get("product_cost_total") is not None
                ]
                rows.sort(
                    key=lambda row: abs(float(row.get("product_cost_total") or 0)),
                    reverse=True,
                )
            else:
                rows = [row for row in self._profit_rows if row.get("profit") is not None]
                rows.sort(key=lambda row: float(row.get("profit") or 0), reverse=True)
            self._populate_profit_sku_rows(rows)
            self.profit_notebook.select(0)
            self.profit_metric_filter_var.set(
                f"当前筛选：{titles[metric]} · 显示 {len(rows)} 个对应 SKU"
            )
            return

        date_from, date_to = self._profit_unallocated_date_range
        breakdown = self.database.monthly_finance_breakdown(date_from, date_to)

        def is_adjustment(row: dict[str, Any]) -> bool:
            api_name = str(row.get("api_name") or "")
            normalized = api_name.casefold()
            return api_name.startswith("Accrual") or "decompensation" in normalized

        if metric == "other_adjustments":
            detail_rows = [row for row in breakdown if is_adjustment(row)]
        elif metric == "sales_returns":
            detail_rows = [
                row for row in breakdown if row.get("category") in {"sales", "returns"}
            ]
            detail_total = sum(float(row.get("amount") or 0) for row in detail_rows)
            expected_total = float(self._profit_summary.get("sales_returns") or 0)
            difference = expected_total - detail_total
            if abs(difference) >= 0.01:
                detail_rows.append({
                    "category": "sales_returns_reconciliation",
                    "category_label": "Finance 销售与退货",
                    "name": "现金流汇总口径补差",
                    "api_name": "cash_flows.orders_amount + returns_amount",
                    "rows_count": 0,
                    "amount": difference,
                })
        elif metric == "accrual_fees":
            detail_rows = [
                row for row in breakdown
                if row.get("category") not in {"sales", "returns"} and not is_adjustment(row)
            ]
        else:
            detail_rows = list(breakdown)
        detail_rows.sort(key=lambda row: abs(float(row.get("amount") or 0)), reverse=True)
        self._clear_tree(self.profit_metric_tree)
        total = 0.0
        records = 0
        for index, row in enumerate(detail_rows):
            amount = float(row.get("amount") or 0)
            total += amount
            records += int(row.get("rows_count") or 0)
            self.profit_metric_tree.insert("", "end", values=(
                titles[metric], row.get("category_label") or row.get("category") or "—",
                row.get("name") or "—", row.get("api_name") or "—",
                _fmt_int(row.get("rows_count")), _fmt_money(amount),
            ), tags=("odd",) if index % 2 else ())
        self.profit_notebook.select(2)
        self.profit_metric_filter_var.set(
            f"当前筛选：{titles[metric]} · {len(detail_rows)} 个 API 费用字段 · "
            f"{records} 条记录 · 合计 {_fmt_money(total)} ₽"
        )

    def clear_profit_metric_filter(self) -> None:
        self._profit_active_metric = ""
        for card in self.profit_metric_cards.values():
            card.configure(highlightbackground=PALETTE.border, highlightthickness=1)
        self._populate_profit_sku_rows(self._profit_rows)
        self._clear_tree(self.profit_metric_tree)
        self.profit_notebook.select(0)
        self.profit_metric_filter_var.set(
            "已清除指标筛选；点击上方指标卡可再次查看对应数据"
        )

    def _profit_tab_changed(self, _event: tk.Event[Any] | None = None) -> None:
        try:
            if self.profit_notebook.index("current") == 1:
                self._load_profit_unallocated_rows()
        except tk.TclError:
            return

    def _load_profit_unallocated_rows(self) -> None:
        if self._profit_unallocated_loaded:
            return
        date_from, date_to = self._profit_unallocated_date_range
        if not date_from or not date_to:
            return
        rows = self.database.monthly_unallocated_finance_rows(date_from, date_to)
        self._clear_tree(self.profit_unallocated_tree)
        for index, row in enumerate(rows):
            self.profit_unallocated_tree.insert("", "end", values=(
                row["operation_date"], row["operation_id"], row["category_label"],
                row["name"] or "—", row["api_name"] or "—", _fmt_money(row["amount"]),
                row["allocation_status"], row["item_skus"] or "—",
                row["posting_number"] or "—", row["operation_type_name"] or "—",
            ), tags=("odd",) if index % 2 else ())
        self._profit_unallocated_loaded = True
        self.profit_unallocated_hint.configure(
            text=f"已逐项列出 {len(rows)} 条费用明细；同一应计记录包含多个费用项目时会拆成多行。"
                 "这些金额只用于店铺级对账，不会重复计入 SKU。"
        )

    def _change_profit_month(self, delta: int) -> None:
        try:
            current, _ = _month_bounds(self.profit_month_var.get())
        except ValueError:
            current = date.today().replace(day=1)
        month_index = current.year * 12 + current.month - 1 + delta
        target = date(month_index // 12, month_index % 12 + 1, 1)
        self.profit_month_var.set(target.strftime("%Y-%m"))
        self.refresh_profit()

    def open_profit_store_detail(self) -> None:
        try:
            month_start, month_end = _month_bounds(self.profit_month_var.get())
        except ValueError as error:
            messagebox.showwarning("月份错误", str(error), parent=self)
            return
        detail = self.database.monthly_finance_store_detail(
            month_start.isoformat(), month_end.isoformat()
        )
        summary = detail["summary"]
        window = tk.Toplevel(self)
        window.title(f"店铺费用与对账 · {self.profit_month_var.get()}")
        window.geometry("1240x760")
        window.minsize(960, 620)
        window.configure(bg=PALETTE.bg)
        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=22, pady=(18, 10))
        tk.Label(
            header, text=f"{self.profit_month_var.get()} 店铺费用与对账",
            font=("Microsoft YaHei UI", 17, "bold"), fg=PALETTE.text, bg=PALETTE.bg,
        ).pack(anchor="w")
        difference = summary.get("reconciliation_difference")
        tk.Label(
            header,
            text=(
                f"逐笔应计净额 {_fmt_money(summary['transaction_total'])} ₽  ·  "
                f"cash_flows 原字段合计 {_fmt_money(summary['cash_flow_reported_total'])} ₽  ·  "
                f"原字段差额 {_fmt_money(difference)} ₽  ·  "
                f"未分摊 {summary['unallocated_operations']} 条 / "
                f"{_fmt_money(summary['unallocated_amount'])} ₽"
            ) if difference is not None else (
                f"逐笔应计 {_fmt_money(summary['transaction_total'])} ₽  ·  "
                f"未分摊 {summary['unallocated_operations']} 条 / "
                f"{_fmt_money(summary['unallocated_amount'])} ₽；尚未同步商店经济汇总"
            ),
            font=("Microsoft YaHei UI", 9), fg=PALETTE.muted, bg=PALETTE.bg,
        ).pack(anchor="w", pady=(5, 0))

        notebook = ttk.Notebook(window)
        notebook.pack(fill="both", expand=True, padx=22, pady=(0, 18))
        breakdown_tab = tk.Frame(notebook, bg="#FFFFFF")
        unallocated_tab = tk.Frame(notebook, bg="#FFFFFF")
        raw_tab = tk.Frame(notebook, bg="#FFFFFF")
        notebook.add(breakdown_tab, text="全部费用分类")
        notebook.add(unallocated_tab, text="店铺级/多 SKU 未分摊")
        notebook.add(raw_tab, text="商店经济原始 API 数据")
        breakdown_tree = self._tree(breakdown_tab, [
            ("category", "费用大类", 145, "w"), ("name", "费用项目", 260, "w"),
            ("api_name", "API 字段/服务名", 350, "w"),
            ("rows", "记录数", 85, "e"), ("amount", "金额", 130, "e"),
        ])
        for index, item in enumerate(detail["breakdown"]):
            breakdown_tree.insert("", "end", values=(
                item["category_label"], item["name"], item["api_name"],
                _fmt_int(item["rows_count"]), _fmt_money(item["amount"]),
            ), tags=("odd",) if index % 2 else ())
        unallocated_tree = self._tree(unallocated_tab, [
            ("date", "发生时间", 145, "w"), ("id", "应计费用 ID", 155, "w"),
            ("type", "业务类型", 185, "w"), ("name", "业务名称", 210, "w"),
            ("posting", "订单/包裹号", 145, "w"), ("skus", "涉及 SKU", 190, "w"),
            ("status", "未分摊原因", 185, "w"), ("amount", "应计净额", 105, "e"),
            ("services", "服务费用明细", 340, "w"),
        ])
        for index, operation in enumerate(detail["unallocated_operations"]):
            unallocated_tree.insert("", "end", values=(
                operation["operation_date"], operation["operation_id"],
                operation["operation_type"], operation["operation_type_name"],
                operation["posting_number"], operation["item_skus"] or "—",
                operation["allocation_status"], _fmt_money(operation["amount"]),
                operation["services_text"] or "—",
            ), tags=("odd",) if index % 2 else ())
        raw_text = tk.Text(
            raw_tab, wrap="none", relief="flat", bg="#F8FAFD", fg=PALETTE.text,
            font=("Consolas", 9), padx=12, pady=10,
        )
        raw_y = ttk.Scrollbar(raw_tab, orient="vertical", command=raw_text.yview)
        raw_x = ttk.Scrollbar(raw_tab, orient="horizontal", command=raw_text.xview)
        raw_text.configure(yscrollcommand=raw_y.set, xscrollcommand=raw_x.set)
        raw_tab.grid_rowconfigure(0, weight=1)
        raw_tab.grid_columnconfigure(0, weight=1)
        raw_text.grid(row=0, column=0, sticky="nsew")
        raw_y.grid(row=0, column=1, sticky="ns")
        raw_x.grid(row=1, column=0, sticky="ew")
        raw_text.insert("1.0", json.dumps({
            "cash_flows": detail["cash_flows"],
            "details": detail["cash_flow_details"],
        }, ensure_ascii=False, indent=2))
        raw_text.configure(state="disabled")

    def sync_profit_accruals(self) -> None:
        try:
            month_start, month_end = _month_bounds(self.profit_month_var.get())
        except ValueError as error:
            messagebox.showwarning("月份错误", str(error), parent=self)
            return
        effective_end = min(month_end, date.today())
        if month_start > effective_end:
            messagebox.showwarning("月份错误", "不能同步未来月份的应计费用。", parent=self)
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.sync_finance_accruals(
                month_start.isoformat(), effective_end.isoformat(), progress,
                self._cancel_event, event_listener,
            )

        self._run_task("正在从 Seller API 同步商店经济与逐笔应计费用…", action,
                       self._profit_accruals_synced)

    def sync_profit_sales(self) -> None:
        try:
            month_start, month_end = _month_bounds(self.profit_month_var.get())
        except ValueError as error:
            messagebox.showwarning("月份错误", str(error), parent=self)
            return
        effective_end = min(month_end, date.today() - timedelta(days=1))
        if month_start > effective_end:
            messagebox.showwarning("月份错误", "尚无已结束日期可同步。", parent=self)
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.sync_monthly_sales(
                month_start.isoformat(), effective_end.isoformat(), progress,
                self._cancel_event, event_listener,
            )

        self._run_task(
            "正在补齐本月 Seller 订单金额与已订购商品…",
            action, self._profit_sales_synced,
        )

    def _profit_sales_synced(self, result: dict[str, Any]) -> None:
        self.refresh_profit()
        self.refresh_logs()
        status = result.get("status")
        if status == "success":
            self.status_var.set(
                f"本月销量已补齐：{_fmt_int(result.get('ordered_products'))} 件，"
                f"订单金额 {_fmt_money(result.get('order_amount'))} ₽"
            )
        elif status == "cooldown":
            self.status_var.set(str(result.get("message") or "Seller 接口正在限频冷却"))
        else:
            self.status_var.set("本月销量已完整缓存，本次未调用 Ozon 接口")

    def _profit_accruals_synced(self, result: dict[str, Any]) -> None:
        self.refresh_profit()
        self.refresh_logs()
        summary = result.get("summary", {})
        self.status_var.set(
            f"应计费用已更新：{result.get('transaction_rows', 0)} 条逐笔记录，"
            f"{summary.get('unallocated_operations', 0)} 条店铺级/多 SKU 记录保留未分摊"
        )

    def open_cost_editor(self, _event: tk.Event[Any] | None = None) -> None:
        selection = self.cost_tree.selection() if hasattr(self, "cost_tree") else ()
        if not selection:
            return
        row = self._cost_rows_by_iid.get(selection[0])
        if row:
            self._show_product_cost_editor(row, cross_border=False)

    def open_cross_border_cost_editor(self, _event: tk.Event[Any] | None = None) -> None:
        selection = self.cross_border_tree.selection() if hasattr(self, "cross_border_tree") else ()
        if not selection:
            return
        row = self._cross_border_rows_by_iid.get(selection[0])
        if row:
            self._show_product_cost_editor(row, cross_border=True)

    def _show_product_cost_editor(
        self, row: dict[str, Any], *, cross_border: bool,
    ) -> None:
        sku = str(row.get("sku") or "")
        if not sku:
            return
        window = tk.Toplevel(self)
        window.title(f"产品成本 · {row.get('offer_id') or '无货号'} / {sku}")
        window.geometry("760x600")
        window.minsize(680, 540)
        window.configure(bg=PALETTE.bg)
        window.transient(self)

        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=22, pady=(18, 12))
        tk.Label(
            header, text=f"{row.get('offer_id') or '无货号'}  /  {sku}",
            font=("Microsoft YaHei UI", 16, "bold"), fg=PALETTE.text, bg=PALETTE.bg,
        ).pack(anchor="w")
        tk.Label(
            header,
            text=f"当前店铺：{self.active_shop.display_name} · 1 CNY = {self.database.rub_per_cny():g} RUB",
            font=("Microsoft YaHei UI", 8), fg=PALETTE.muted, bg=PALETTE.bg,
        ).pack(anchor="w", pady=(4, 0))

        panel = self._panel(window)
        panel.pack(fill="both", expand=True, padx=22, pady=(0, 18))
        self._panel_title(
            panel, "采购成本、头程与产品参数",
            "采购成本按人民币保存；头程直接按卢布保存，不再随汇率变化。",
        )
        form = tk.Frame(panel, bg="#FFFFFF")
        form.pack(fill="x", padx=18, pady=(0, 10))
        variables = {
            "unit": tk.StringVar(value="" if row.get("unit_cost_cny") is None else str(float(row["unit_cost_cny"]))),
            "first": tk.StringVar(value="" if row.get("first_mile_cost") is None else str(float(row["first_mile_cost"]))),
            "length": tk.StringVar(value="" if row.get("length_cm") is None else str(float(row["length_cm"]))),
            "width": tk.StringVar(value="" if row.get("width_cm") is None else str(float(row["width_cm"]))),
            "height": tk.StringVar(value="" if row.get("height_cm") is None else str(float(row["height_cm"]))),
            "weight": tk.StringVar(value="" if row.get("weight_kg") is None else str(float(row["weight_kg"]))),
            "note": tk.StringVar(value=str(row.get("note") or "")),
        }
        fields = [
            ("采购成本（CNY）", "unit"), ("头程（RUB）", "first"),
            ("长度（cm）", "length"), ("宽度（cm）", "width"),
            ("高度（cm）", "height"), ("重量（kg）", "weight"),
        ]
        for index, (label, key) in enumerate(fields):
            row_index, column = divmod(index, 2)
            block = tk.Frame(form, bg="#FFFFFF")
            block.grid(row=row_index, column=column, sticky="ew", padx=(0, 18), pady=7)
            form.grid_columnconfigure(column, weight=1)
            tk.Label(block, text=label, width=15, anchor="w", bg="#FFFFFF",
                     fg=PALETTE.muted).pack(side="left")
            ttk.Entry(block, textvariable=variables[key]).pack(side="left", fill="x", expand=True)
        note_block = tk.Frame(form, bg="#FFFFFF")
        note_block.grid(row=3, column=0, columnspan=2, sticky="ew", padx=(0, 18), pady=7)
        tk.Label(note_block, text="备注", width=15, anchor="w", bg="#FFFFFF",
                 fg=PALETTE.muted).pack(side="left")
        ttk.Entry(note_block, textvariable=variables["note"]).pack(side="left", fill="x", expand=True)

        preview_var = tk.StringVar()
        tk.Label(
            panel, textvariable=preview_var, anchor="w", bg="#FFFFFF", fg=PALETTE.primary,
            font=("Microsoft YaHei UI", 8, "bold"),
        ).pack(fill="x", padx=18, pady=(0, 8))

        def read_values() -> dict[str, float | None] | None:
            try:
                return {
                    "unit": _optional_nonnegative_float(variables["unit"].get(), "采购成本"),
                    "first": _optional_nonnegative_float(variables["first"].get(), "头程"),
                    "length": _optional_nonnegative_float(variables["length"].get(), "长度"),
                    "width": _optional_nonnegative_float(variables["width"].get(), "宽度"),
                    "height": _optional_nonnegative_float(variables["height"].get(), "高度"),
                    "weight": _optional_nonnegative_float(variables["weight"].get(), "重量"),
                }
            except ValueError as error:
                messagebox.showwarning("格式错误", str(error), parent=window)
                return None

        selling_cny = float(row.get("selling_price_cny") or 0)
        if cross_border and selling_cny:
            preview_var.set(
                f"本期单件售价约 {selling_cny:.2f} CNY；保存重量后将按跨境 IFS 公式计算运费。"
            )
        else:
            preview_var.set("尺寸和重量用于跨境运费核算；本土月度盈亏使用采购成本与头程。")

        action_bar = tk.Frame(panel, bg="#FFFFFF")
        action_bar.pack(fill="x", padx=18, pady=(2, 12))

        def after_save(message: str) -> None:
            self.refresh_costs()
            self.refresh_profit()
            if hasattr(self, "product_profit_tree"):
                self.refresh_product_profit()
            if self.current_page == "cross_border":
                self.refresh_cross_border()
            self.status_var.set(message)

        def save_one() -> None:
            values = read_values()
            if values is None:
                return
            self.database.save_product_cost_mixed(
                sku, values["unit"], values["first"],
                length_cm=values["length"], width_cm=values["width"],
                height_cm=values["height"], weight_kg=values["weight"],
                note=variables["note"].get().strip(),
            )
            after_save(f"{row.get('offer_id') or sku} 的采购成本、卢布头程和产品参数已保存。")
            window.destroy()

        self._button(action_bar, "保存此 SKU", save_one, compact=True).pack(side="left")

        family_frame = tk.Frame(panel, bg="#F8FAFC")
        family_frame.pack(fill="x", padx=18, pady=(0, 14))
        default_family = self._cost_family_prefix(str(row.get("offer_id") or ""))
        family_values = tuple(sorted({
            prefix for item in self.database.product_sync_rows()
            if (prefix := self._cost_family_prefix(str(item.get("offer_id") or "")))
        }, key=str.casefold))
        family_var = tk.StringVar(value=default_family)
        family_status = tk.StringVar()
        tk.Label(family_frame, text="批量系列", bg="#F8FAFC", fg=PALETTE.muted).pack(side="left", padx=(10, 0), pady=10)
        family_combo = ttk.Combobox(
            family_frame, state="normal", width=20, textvariable=family_var,
            values=family_values,
        )
        family_combo.pack(side="left", padx=8, pady=8)

        def targets() -> list[dict[str, Any]]:
            return self.database.find_product_cost_targets(family_var.get(), prefix=True)

        def update_status(_event: tk.Event[Any] | None = None) -> None:
            family_status.set(f"匹配 {len(targets())} 个 SKU")

        def filter_family(event: tk.Event[Any]) -> None:
            self._filter_combo_keyrelease(event, family_combo, family_values, family_var)
            update_status()

        def save_family() -> None:
            values = read_values()
            matches = targets()
            if values is None:
                return
            if not family_var.get().strip() or not matches:
                messagebox.showwarning("没有匹配", "请输入类似 GJYB001 的系列货号。", parent=window)
                return
            if not messagebox.askyesno(
                "统一系列成本",
                f"将采购成本、头程、长宽高和重量同步到 {len(matches)} 个匹配 SKU。是否继续？",
                parent=window,
            ):
                return
            saved = self.database.save_product_costs_mixed(
                (str(item.get("sku") or "") for item in matches),
                values["unit"], values["first"], variables["note"].get().strip(),
                length_cm=values["length"], width_cm=values["width"],
                height_cm=values["height"], weight_kg=values["weight"],
            )
            after_save(
                f"系列 {family_var.get()} 已统一 {saved} 个 SKU 的成本、头程、尺寸和重量。"
            )
            update_status()

        family_combo.bind("<KeyRelease>", filter_family)
        family_combo.bind("<<ComboboxSelected>>", update_status)
        self._button(family_frame, "统一匹配 SKU 成本", save_family,
                     compact=True).pack(side="left", pady=8)
        tk.Label(family_frame, textvariable=family_status, bg="#F8FAFC",
                 fg=PALETTE.primary).pack(side="left", padx=10)
        update_status()

    def open_profit_sku_detail(self, event: tk.Event[Any] | None = None) -> None:
        if event is not None and getattr(event, "num", None) == 1:
            iid = self.profit_tree.identify_row(event.y)
            if iid:
                self.profit_tree.selection_set(iid)
        selected = self.profit_tree.selection()
        if not selected:
            return
        sku = str(selected[0])
        row = self._profit_rows_by_sku.get(sku)
        if not row:
            return
        month_start, month_end = _month_bounds(self.profit_month_var.get())
        detail = self.database.monthly_finance_detail(
            sku, month_start.isoformat(), month_end.isoformat()
        )
        window = tk.Toplevel(self)
        window.title(f"SKU 应计费用详情 · {row.get('offer_id') or '无货号'} / {sku}")
        window.geometry("1220x760")
        window.minsize(940, 620)
        window.configure(bg=PALETTE.bg)
        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=22, pady=(18, 10))
        tk.Label(
            header, text=f"{row.get('offer_id') or '无货号'}  /  {sku}",
            font=("Microsoft YaHei UI", 17, "bold"), fg=PALETTE.text, bg=PALETTE.bg,
        ).pack(side="left")
        tk.Label(
            header, text=f"{month_start.isoformat()} 至 {month_end.isoformat()} · "
                         f"{len(detail['operations'])} 条精确归属记录",
            font=("Microsoft YaHei UI", 9), fg=PALETTE.muted, bg=PALETTE.bg,
        ).pack(side="right")

        cost_panel = self._panel(window)
        cost_panel.pack(fill="x", padx=22, pady=(0, 10))
        cost_bar = tk.Frame(cost_panel, bg="#FFFFFF")
        cost_bar.pack(fill="x", padx=16, pady=11)
        unit_var = tk.StringVar(
            value="" if row.get("unit_cost_cny") is None else str(float(row["unit_cost_cny"]))
        )
        first_mile_var = tk.StringVar(
            value="" if row.get("first_mile_cost") is None else str(float(row["first_mile_cost"]))
        )
        note_var = tk.StringVar(value=str(row.get("note") or ""))
        for label_text, variable, width in (
            ("单件成本 CNY", unit_var, 10), ("单件头程 RUB", first_mile_var, 10),
            ("备注", note_var, 34),
        ):
            tk.Label(cost_bar, text=label_text, bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
            ttk.Entry(cost_bar, width=width, textvariable=variable).pack(side="left", padx=(6, 15))

        def read_cost_values() -> tuple[float | None, float | None] | None:
            try:
                unit_cost = _optional_nonnegative_float(unit_var.get(), "单件成本")
                first_mile = _optional_nonnegative_float(first_mile_var.get(), "单件头程")
            except ValueError as error:
                messagebox.showwarning("成本格式错误", str(error), parent=window)
                return None
            if (unit_cost is None) != (first_mile is None):
                messagebox.showwarning(
                    "成本未填完整", "单件成本和单件头程需要同时填写，或同时留空。",
                    parent=window,
                )
                return None
            return unit_cost, first_mile

        def save_cost() -> None:
            values = read_cost_values()
            if values is None:
                return
            unit_cost, first_mile = values
            self.database.save_product_cost_mixed(
                sku, unit_cost, first_mile,
                length_cm=row.get("length_cm"), width_cm=row.get("width_cm"),
                height_cm=row.get("height_cm"), weight_kg=row.get("weight_kg"),
                note=note_var.get().strip(),
            )
            self.refresh_profit()
            self.refresh_costs()
            window.destroy()
            self.status_var.set("SKU 成本、头程与备注已保存到本地")

        self._button(cost_bar, "仅保存此 SKU", save_cost, compact=True).pack(side="left")
        tk.Label(
            cost_bar,
            text=f"应计净额 {_fmt_money(row['platform_payout'])} ₽  ·  "
                 f"佣金 {_fmt_money(row['commission'])} ₽  ·  "
                 f"广告 {_fmt_money(row['ad_spend'])} ₽  ·  "
                 f"物流 {_fmt_money(row['logistics'])} ₽",
            bg="#FFFFFF", fg=PALETTE.text, font=("Microsoft YaHei UI", 9, "bold"),
        ).pack(side="right")

        family_bar = tk.Frame(cost_panel, bg="#FFFFFF")
        family_bar.pack(fill="x", padx=16, pady=(0, 11))
        default_family = self._cost_family_prefix(str(row.get("offer_id") or ""))
        family_values = tuple(sorted({
            prefix for item in self.database.product_sync_rows()
            if (prefix := self._cost_family_prefix(str(item.get("offer_id") or "")))
        }, key=str.casefold))
        family_var = tk.StringVar(value=default_family)
        family_status_var = tk.StringVar()
        tk.Label(
            family_bar, text="同系列货号", bg="#FFFFFF", fg=PALETTE.muted,
        ).pack(side="left")
        family_combo = ttk.Combobox(
            family_bar, state="normal", width=18, textvariable=family_var,
            values=family_values,
        )
        family_combo.pack(side="left", padx=(6, 8))

        def family_targets() -> list[dict[str, Any]]:
            return self.database.find_product_cost_targets(
                family_var.get().strip(), prefix=True,
            )

        def update_family_status() -> None:
            targets = family_targets()
            sample = "、".join(
                str(item.get("offer_id") or item.get("sku") or "")
                for item in targets[:4]
            )
            family_status_var.set(
                f"匹配 {len(targets)} 个 SKU"
                + (f"：{sample}" if sample else "")
                + ("…" if len(targets) > 4 else "")
            )

        def filter_families(event: tk.Event[Any]) -> None:
            self._filter_combo_keyrelease(event, family_combo, family_values, family_var)
            update_family_status()

        def save_family_cost() -> None:
            values = read_cost_values()
            if values is None:
                return
            unit_cost, first_mile = values
            prefix = family_var.get().strip()
            targets = family_targets()
            if not prefix or not targets:
                messagebox.showwarning(
                    "没有匹配商品", "请输入系列货号，例如 GJYB001，并从列表确认匹配结果。",
                    parent=window,
                )
                return
            sample = "、".join(
                str(item.get("offer_id") or item.get("sku") or "")
                for item in targets[:8]
            )
            action_text = (
                f"人民币单件成本 {unit_cost:g}、卢布单件头程 {first_mile:g}"
                if unit_cost is not None and first_mile is not None
                else "清空成本和头程"
            )
            if not messagebox.askyesno(
                "确认同步同系列成本",
                f"系列前缀：{prefix}\n匹配：{len(targets)} 个 SKU\n"
                f"商品：{sample}{'…' if len(targets) > 8 else ''}\n\n"
                f"将{action_text}同步到以上全部 SKU。是否继续？",
                parent=window,
            ):
                return
            saved = self.database.save_product_costs_mixed(
                (str(item.get("sku") or "") for item in targets),
                unit_cost, first_mile,
            )
            self.refresh_profit()
            update_family_status()
            self.status_var.set(
                f"已将系列 {prefix} 的成本与头程同步到 {saved} 个 SKU"
            )
            messagebox.showinfo(
                "批量保存完成", f"已更新 {saved} 个 SKU；其他系列未修改。", parent=window,
            )

        family_combo.bind("<KeyRelease>", filter_families)
        family_combo.bind("<<ComboboxSelected>>", lambda _event: update_family_status())
        self._button(
            family_bar, "同步同系列成本与头程", save_family_cost, compact=True,
        ).pack(side="left")
        tk.Label(
            family_bar, textvariable=family_status_var, bg="#FFFFFF", fg=PALETTE.primary,
            font=("Microsoft YaHei UI", 8),
        ).pack(side="left", padx=10)
        update_family_status()

        notebook = ttk.Notebook(window)
        notebook.pack(fill="both", expand=True, padx=22, pady=(0, 18))
        category_tab = tk.Frame(notebook, bg="#FFFFFF")
        operations_tab = tk.Frame(notebook, bg="#FFFFFF")
        raw_tab = tk.Frame(notebook, bg="#FFFFFF")
        notebook.add(category_tab, text="费用分类")
        notebook.add(operations_tab, text="逐笔应计明细")
        notebook.add(raw_tab, text="原始 API 数据")
        category_tree = self._tree(category_tab, [
            ("category", "费用大类", 135, "w"), ("name", "费用项目", 250, "w"),
            ("api_name", "API 字段/服务名", 310, "w"),
            ("rows", "记录数", 80, "e"), ("amount", "金额", 120, "e"),
        ])
        for index, item in enumerate(detail["categories"]):
            category_tree.insert("", "end", values=(
                item["category_label"], item["name"], item["api_name"],
                _fmt_int(item["rows_count"]), _fmt_money(item["amount"]),
            ), tags=("odd",) if index % 2 else ())
        operation_tree = self._tree(operations_tab, [
            ("date", "发生时间", 145, "w"), ("id", "应计费用 ID", 150, "w"),
            ("type", "业务类型", 175, "w"), ("name", "业务名称", 190, "w"),
            ("posting", "订单/包裹号", 145, "w"), ("amount", "应计净额", 95, "e"),
            ("sale", "销售应计", 95, "e"), ("commission", "佣金", 90, "e"),
            ("delivery", "配送汇总字段", 105, "e"),
            ("return_delivery", "退货配送汇总", 105, "e"),
            ("services", "服务费用明细", 360, "w"),
        ])
        for index, operation in enumerate(detail["operations"]):
            operation_tree.insert("", "end", values=(
                operation["operation_date"], operation["operation_id"],
                operation["operation_type"], operation["operation_type_name"],
                operation["posting_number"], _fmt_money(operation["amount"]),
                _fmt_money(operation["accruals_for_sale"]),
                _fmt_money(operation["sale_commission"]),
                _fmt_money(operation["delivery_charge"]),
                _fmt_money(operation["return_delivery_charge"]),
                operation["services_text"] or "—",
            ), tags=("odd",) if index % 2 else ())
        raw_text = tk.Text(
            raw_tab, wrap="none", relief="flat", bg="#F8FAFD", fg=PALETTE.text,
            font=("Consolas", 9), padx=12, pady=10,
        )
        raw_y = ttk.Scrollbar(raw_tab, orient="vertical", command=raw_text.yview)
        raw_x = ttk.Scrollbar(raw_tab, orient="horizontal", command=raw_text.xview)
        raw_text.configure(yscrollcommand=raw_y.set, xscrollcommand=raw_x.set)
        raw_tab.grid_rowconfigure(0, weight=1)
        raw_tab.grid_columnconfigure(0, weight=1)
        raw_text.grid(row=0, column=0, sticky="nsew")
        raw_y.grid(row=0, column=1, sticky="ns")
        raw_x.grid(row=1, column=0, sticky="ew")
        raw_text.insert("1.0", json.dumps(
            [operation["raw"] for operation in detail["operations"]],
            ensure_ascii=False, indent=2,
        ))
        raw_text.configure(state="disabled")

    def save_selected_product_cost(self) -> None:
        """Backward-compatible entry point; the editor now lives in the SKU dialog."""
        try:
            self.open_profit_sku_detail()
        except tk.TclError:
            return

    def refresh_weekly(self) -> None:
        if not hasattr(self, "weekly_tree"):
            return
        try:
            chosen = date.fromisoformat(self.weekly_end_var.get().strip())
        except ValueError:
            messagebox.showwarning("日期错误", "周报结束日格式应为 YYYY-MM-DD。", parent=self)
            return
        chosen -= timedelta(days=(chosen.weekday() + 1) % 7)
        latest = _latest_completed_week_end()
        if chosen > latest:
            chosen = latest
        self.weekly_end_var.set(chosen.isoformat())
        report = self.database.weekly_report(chosen.isoformat())
        self._clear_tree(self.weekly_tree)
        summary_rows = [
            (report["previous_start"], report["previous_end"], "上期", report["previous"], False),
            (report["current_start"], report["current_end"], "本期", report["current"], False),
            ("", "", "环比", report["comparison"], True),
        ]
        for index, (start, end, label, values, comparison) in enumerate(summary_rows):
            if comparison:
                formatted = [
                    _fmt_change(values.get("revenue")), _fmt_change(values.get("orders")),
                    _fmt_change(values.get("spend")), _fmt_change(values.get("ad_orders")),
                    "—", _fmt_change(values.get("ad_order_share")),
                    _fmt_change(values.get("acots")), "—",
                ]
                period = ""
            else:
                formatted = [
                    _fmt_money(values.get("revenue", 0)), _fmt_int(values.get("orders", 0)),
                    _fmt_money(values.get("spend", 0)), _fmt_int(values.get("ad_orders", 0)),
                    "—", f"{float(values.get('ad_order_share') or 0):.2f}%",
                    f"{float(values.get('acots') or 0):.2f}%", "—",
                ]
                period = f"{start[5:].replace('-', '/')}–{end[5:].replace('-', '/')}"
            self.weekly_tree.insert("", "end", values=(period, label, *formatted),
                                    tags=("odd",) if index % 2 else ())

        self._clear_tree(self.weekly_daily_tree)
        weekdays = ("星期一", "星期二", "星期三", "星期四", "星期五", "星期六", "星期日")
        for index, row in enumerate(report["daily"]):
            day_value = date.fromisoformat(str(row["day"]))
            self.weekly_daily_tree.insert("", "end", values=(
                row["day"], weekdays[day_value.weekday()], _fmt_money(row["revenue"]),
                _fmt_int(row["orders"]), _fmt_money(row["spend"]), _fmt_int(row["ad_orders"]),
                "—", f"{float(row['ad_order_share'] or 0):.2f}%",
                f"{float(row['acots'] or 0):.2f}%", "—",
            ), tags=("odd",) if index % 2 else ())
        current, comparison = report["current"], report["comparison"]
        auto_text = "已开启" if self.weekly_auto_var.get() else "已关闭"
        self.weekly_hint.configure(
            text=(
                f"本期销售额 {_fmt_money(current['revenue'])} ₽，销量 {_fmt_int(current['orders'])}，"
                f"销售额环比 {_fmt_change(comparison.get('revenue'))}，"
                f"ACoTS {float(current['acots'] or 0):.2f}%。"
                f"  自动周更{auto_text}：软件打开时会检查并补齐最新完整周。"
                "  库存/可售天数需要库存快照历史，当前先显示“—”。"
            )
        )

    def _change_week(self, days: int) -> None:
        try:
            current = date.fromisoformat(self.weekly_end_var.get().strip())
        except ValueError:
            current = _latest_completed_week_end()
        target = current + timedelta(days=days)
        if target > _latest_completed_week_end():
            target = _latest_completed_week_end()
        self.weekly_end_var.set(target.isoformat())
        self.refresh_weekly()

    def _select_latest_week(self) -> None:
        self.weekly_end_var.set(_latest_completed_week_end().isoformat())
        self.refresh_weekly()

    def _toggle_weekly_auto(self) -> None:
        self.database.save_settings({
            "weekly_auto_sync_enabled": "1" if self.weekly_auto_var.get() else "0"
        })
        self.status_var.set("每周自动拉取已开启" if self.weekly_auto_var.get() else "每周自动拉取已关闭")
        if self.weekly_auto_var.get():
            self._schedule_weekly_auto_check(250)

    def _schedule_weekly_auto_check(self, delay_ms: int = 3_600_000) -> None:
        previous = getattr(self, "_weekly_timer_id", None)
        if previous:
            try:
                self.after_cancel(previous)
            except tk.TclError:
                pass
        self._weekly_timer_id = self.after(delay_ms, self._check_weekly_auto_sync)

    def _check_weekly_auto_sync(self) -> None:
        self._weekly_timer_id = None
        self._schedule_weekly_auto_check()
        if not self.weekly_auto_var.get() or self._task_running:
            return
        target = _latest_completed_week_end()
        if self.database.get_setting("weekly_last_synced_end") >= target.isoformat():
            self._maybe_start_auto_feishu_weekly(target)
            return
        last_attempt_text = self.database.get_setting("weekly_auto_last_attempt_at")
        if last_attempt_text:
            try:
                if datetime.now() - datetime.fromisoformat(last_attempt_text) < timedelta(hours=6):
                    return
            except ValueError:
                pass
        week_start = target - timedelta(days=6)
        cache = self.database.sales_sync_state(week_start.isoformat(), target.isoformat())
        seller_verified = str(cache.get("verified_through") or "") >= target.isoformat()
        credentials = self.service.load_credentials()
        has_seller = bool(credentials.seller_client_id and credentials.seller_api_key)
        has_performance = bool(
            credentials.performance_client_id and credentials.performance_client_secret
        )
        if (not seller_verified and not has_seller) or not has_performance:
            self.status_var.set("周报自动更新等待尚未缓存数据所需的 API 凭证")
            return
        self.database.save_settings({
            "weekly_auto_last_attempt_at": datetime.now().isoformat(timespec="seconds")
        })
        self.weekly_end_var.set(target.isoformat())
        self._sync_week_period(automatic=True)

    def _toggle_shipment_tracking_auto(self) -> None:
        enabled = self.shipment_tracking_auto_var.get()
        self.database.save_settings({
            "shipment_tracking_auto_enabled": "1" if enabled else "0",
        })
        self.status_var.set(
            "发货跟踪自动检查已开启" if enabled else "发货跟踪自动检查已关闭"
        )
        if enabled:
            self._schedule_shipment_auto_check(250)

    def _schedule_shipment_auto_check(self, delay_ms: int = 86_400_000) -> None:
        previous = getattr(self, "_shipment_timer_id", None)
        if previous:
            try:
                self.after_cancel(previous)
            except tk.TclError:
                pass
        self._shipment_timer_id = self.after(delay_ms, self._check_shipment_auto_sync)

    def _check_shipment_auto_sync(self) -> None:
        self._shipment_timer_id = None
        self._schedule_shipment_auto_check()
        if not self.shipment_tracking_auto_var.get() or self._task_running:
            return
        credentials = self.service.load_credentials()
        complete = all((
            credentials.feishu_app_id, credentials.feishu_app_secret,
            credentials.feishu_app_token, credentials.feishu_tracking_table_id,
        ))
        if not complete:
            return
        last_attempt = self.database.get_setting("shipment_tracking_last_attempt_at")
        if last_attempt:
            try:
                if datetime.now() - datetime.fromisoformat(last_attempt) < timedelta(hours=23):
                    return
            except ValueError:
                pass
        self.database.save_settings({
            "shipment_tracking_last_attempt_at": datetime.now().isoformat(timespec="seconds")
        })
        self.start_shipment_tracking_sync(automatic=True)

    def sync_weekly_now(self) -> None:
        self._sync_week_period(automatic=False)

    def _sync_week_period(self, automatic: bool) -> None:
        try:
            end = date.fromisoformat(self.weekly_end_var.get().strip())
        except ValueError:
            if not automatic:
                messagebox.showwarning("日期错误", "周报结束日格式应为 YYYY-MM-DD。", parent=self)
            return
        end -= timedelta(days=(end.weekday() + 1) % 7)
        start = end - timedelta(days=6)
        credentials = self.service.load_credentials()
        cache = self.database.sales_sync_state(start.isoformat(), end.isoformat())
        seller_verified = str(cache.get("verified_through") or "") >= end.isoformat()
        can_seller = bool(credentials.seller_client_id and credentials.seller_api_key)
        can_performance = bool(
            credentials.performance_client_id and credentials.performance_client_secret
        )
        if (not seller_verified and not can_seller) or not can_performance:
            if automatic:
                self.status_var.set("周报自动更新等待完整 API 凭证")
                return
            missing = []
            if not seller_verified and not can_seller:
                missing.append("Seller")
            if not can_performance:
                missing.append("Performance")
            messagebox.showwarning(
                "缺少 API 凭证", "请先在 API 设置中保存 " + "、".join(missing) + " 凭证。",
                parent=self,
            )
            return

        def action() -> list[Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            results: list[Any] = []
            if not seller_verified:
                results.extend(self.service.sync_seller(
                    start.isoformat(), end.isoformat(), progress,
                    self._cancel_event, event_listener,
                ))
            if can_performance:
                results.extend(self.service.sync_performance(
                    start.isoformat(), end.isoformat(), progress,
                    self._cancel_event, event_listener,
                ))
            return results

        callback = lambda results: self._weekly_sync_finished(results, end, automatic)
        self._run_task("正在自动更新周报…" if automatic else "正在拉取周报数据…", action, callback)

    def _weekly_sync_finished(self, results: list[Any], week_end: date, automatic: bool) -> None:
        failed = any("失败" in str(getattr(item, "message", "")) for item in results)
        if not failed:
            self.database.save_settings({"weekly_last_synced_end": week_end.isoformat()})
        self.refresh_all()
        if automatic:
            self.status_var.set(
                f"周报已自动更新至 {week_end.isoformat()}" if not failed
                else "周报自动更新部分失败，请查看数据中心日志"
            )
            if not failed:
                self._maybe_start_auto_feishu_weekly(week_end)
        else:
            message = "\n".join(
                f"{item.source}: {item.rows} 行，{item.message}" for item in results
            ) or "本地 Seller 数据已核验，无需重复请求；广告数据已刷新。"
            if failed:
                messagebox.showwarning("周报更新完成（有提示）", message, parent=self)
            else:
                messagebox.showinfo("周报更新完成", message, parent=self)

    def refresh_inventory_product_options(self) -> None:
        if not hasattr(self, "inventory_product_tree"):
            return
        try:
            target_days = self._inventory_target_days()
        except ValueError as error:
            self.inventory_status_var.set(str(error))
            return
        products = self.database.inventory_products(target_days)
        self._inventory_products_by_display = {}
        for row in products:
            sku = str(row.get("sku") or "").strip()
            if not sku:
                continue
            display = " | ".join(filter(None, (
                str(row.get("offer_id") or "").strip(), sku,
                str(row.get("product_name") or "").strip(),
            )))
            self._inventory_products_by_display[display] = dict(row)
        self._inventory_product_values = tuple(self._inventory_products_by_display)
        query = self.inventory_search_var.get().strip().casefold()
        visible = [
            row for display, row in self._inventory_products_by_display.items()
            if not query or query in display.casefold()
        ]
        self._clear_tree(self.inventory_product_tree)
        self._inventory_products_by_iid = {}
        for index, row in enumerate(visible):
            iid = f"product-{index}"
            self._inventory_products_by_iid[iid] = row
            days = row.get("sellable_days_after")
            self.inventory_product_tree.insert("", "end", iid=iid, values=(
                row.get("offer_id") or "—", row.get("sku") or "—",
                _fmt_int(row.get("available_stock") or 0),
                _fmt_int(row.get("transit_stock") or 0),
                _fmt_int(row.get("requested_stock") or 0),
                f"{float(row.get('daily_sales') or 0):.2f}",
                _fmt_int(round(float(row.get("monthly_estimated") or 0))),
                _fmt_int(row.get("recommended_qty") or 0),
                _fmt_int(row.get("planned_qty") or 0),
                _fmt_int(row.get("planned_clusters") or 0),
                f"{float(days):.1f}" if days is not None else "—",
            ), tags=("odd",) if index % 2 else ())
        totals = {
            "products": len(visible),
            "available": sum(int(row.get("available_stock") or 0) for row in visible),
            "transit": sum(int(row.get("transit_stock") or 0) for row in visible),
            "daily": sum(float(row.get("daily_sales") or 0) for row in visible),
            "monthly": sum(float(row.get("monthly_estimated") or 0) for row in visible),
            "recommended": sum(int(row.get("recommended_qty") or 0) for row in visible),
        }
        for key, label in self.inventory_overview_labels.items():
            value = totals[key]
            label.configure(text=f"{value:.2f}" if key == "daily" else _fmt_int(round(value)))
        cached = sum(1 for row in visible if (
            row.get("available_stock") or row.get("transit_stock")
            or row.get("requested_stock") or row.get("daily_sales")
        ))
        self.inventory_status_var.set(
            f"显示 {len(visible)} / {len(products)} 个商品，其中 {cached} 个已有库存快照。"
        )

    def open_inventory_product_detail(self, event: tk.Event[Any] | None = None) -> None:
        if event is not None and getattr(event, "y", None) is not None:
            iid = self.inventory_product_tree.identify_row(event.y)
            if iid:
                self.inventory_product_tree.selection_set(iid)
                self.inventory_product_tree.focus(iid)
        selected = self.inventory_product_tree.selection()
        product = self._inventory_products_by_iid.get(str(selected[0])) if selected else None
        if not product:
            return
        old_window = getattr(self, "inventory_detail_window", None)
        if old_window is not None and old_window.winfo_exists():
            old_window.destroy()
        display = " | ".join(filter(None, (
            str(product.get("offer_id") or "").strip(),
            str(product.get("sku") or "").strip(),
            str(product.get("product_name") or "").strip(),
        )))
        self.inventory_product_var.set(display)
        self._inventory_products_by_display[display] = product

        window = tk.Toplevel(self)
        self.inventory_detail_window = window
        window.title(f"SKU 库存与补货计划 · {product.get('offer_id') or product.get('sku')}")
        window.geometry("1380x790")
        window.minsize(1100, 650)
        window.configure(bg=PALETTE.bg)
        window.transient(self)
        window.protocol("WM_DELETE_WINDOW", self._close_inventory_product_detail)

        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=26, pady=(20, 12))
        tk.Label(
            header, text=str(product.get("offer_id") or product.get("sku") or "SKU"),
            font=("Microsoft YaHei UI", 19, "bold"), fg=PALETTE.text, bg=PALETTE.bg,
        ).pack(anchor="w")
        tk.Label(
            header,
            text=f"Ozon SKU {product.get('sku')} · {product.get('product_name') or '商品名称未返回'}",
            font=("Microsoft YaHei UI", 9), fg=PALETTE.muted, bg=PALETTE.bg,
        ).pack(anchor="w", pady=(3, 0))

        controls_panel = self._panel(window)
        controls_panel.pack(fill="x", padx=26, pady=(0, 9))
        controls = tk.Frame(controls_panel, bg="#FFFFFF")
        controls.pack(fill="x", padx=15, pady=9)
        tk.Label(controls, text="目标可售天数", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        detail_target = ttk.Entry(controls, width=7, textvariable=self.inventory_target_days_var)
        detail_target.pack(side="left", padx=(6, 9))
        detail_target.bind("<Return>", lambda _event: self.refresh_inventory_plan())
        self._button(controls, "刷新计算", self.refresh_inventory_plan, secondary=True, compact=True).pack(side="left")
        self._button(controls, "同步此 SKU", self.sync_selected_inventory, compact=True).pack(side="left", padx=6)
        self._button(
            controls, "采用全部建议", self.apply_all_inventory_recommendations,
            secondary=True, compact=True,
        ).pack(side="left")
        self._button(
            controls, "导出 Excel", self.export_inventory_plan,
            secondary=True, compact=True,
        ).pack(side="right")
        self._button(
            controls, "转到约仓计划", self.open_supply_for_inventory_product,
            secondary=True, compact=True,
        ).pack(side="right", padx=6)

        cards = tk.Frame(window, bg=PALETTE.bg)
        cards.pack(fill="x", padx=26, pady=(0, 9))
        self.inventory_summary_labels = {}
        for index, (key, title, unit, color) in enumerate([
            ("available", "剩余库存", "件", PALETTE.primary),
            ("transit", "在途", "件", PALETTE.amber),
            ("daily", "日均销量", "件/天", PALETTE.green),
            ("monthly", "月估销量", "件", "#7A5AF8"),
            ("recommended", "建议补货", "件", "#F04438"),
            ("planned", "计划补货", "件", "#0BA5EC"),
        ]):
            card = self._metric_card(cards, title, unit, color, small=True)
            card.grid(row=0, column=index, sticky="nsew", padx=4)
            cards.grid_columnconfigure(index, weight=1)
            self.inventory_summary_labels[key] = card.value_label  # type: ignore[attr-defined]

        editor = self._panel(window)
        editor.pack(fill="x", padx=26, pady=(0, 9))
        edit_row = tk.Frame(editor, bg="#FFFFFF")
        edit_row.pack(fill="x", padx=15, pady=9)
        self.inventory_selected_cluster_label = tk.Label(
            edit_row, text="先选择一个集群", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        )
        self.inventory_selected_cluster_label.pack(side="left", padx=(0, 10))
        tk.Label(edit_row, text="计划补货", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        quantity_entry = ttk.Entry(
            edit_row, width=9, textvariable=self.inventory_planned_qty_var,
        )
        quantity_entry.pack(side="left", padx=(6, 5))
        quantity_entry.bind("<Return>", lambda _event: self.save_inventory_cluster_qty())
        self._button(
            edit_row, "保存数量", self.save_inventory_cluster_qty, compact=True,
        ).pack(side="left")
        tk.Label(
            edit_row, text="也可双击表格中的“计划补货”单元格直接编辑；保存后约仓页自动读取。",
            bg="#FFFFFF", fg=PALETTE.muted, font=("Microsoft YaHei UI", 8),
        ).pack(side="right")

        plan_panel = self._panel(window)
        plan_panel.pack(fill="both", expand=True, padx=26, pady=(0, 18))
        self._panel_title(plan_panel, "全部集群库存与补货计划", "中俄集群名、库存、在途和计划数量")
        self.inventory_tree = self._tree(plan_panel, [
            ("cluster", "集群（俄文）", 175, "w"), ("cluster_cn", "集群中文名", 155, "w"),
            ("macro", "集群 ID", 80, "w"),
            ("available", "库存", 70, "e"), ("transit", "在途", 65, "e"),
            ("requested", "已申请", 65, "e"), ("daily", "日均销量", 80, "e"),
            ("monthly", "月估销量", 85, "e"), ("idc", "IDC", 60, "e"),
            ("recommended", "建议补货", 85, "e"), ("planned", "计划补货", 85, "e"),
            ("days", "补货后可售天数", 115, "e"),
        ])
        self.inventory_tree.bind("<<TreeviewSelect>>", self._inventory_row_selected)
        self.inventory_tree.bind("<Double-1>", self._inventory_tree_double_click)
        self.refresh_inventory_plan()

    def _close_inventory_product_detail(self) -> None:
        window = getattr(self, "inventory_detail_window", None)
        if window is not None and window.winfo_exists():
            window.destroy()
        self.inventory_detail_window = None
        self.refresh_inventory_product_options()

    def refresh_ai_product_options(self) -> None:
        if not hasattr(self, "ai_product_combo"):
            return
        all_label = "全部经营数据（商品/活动 Top 20）"
        previous = self.ai_product_var.get()
        self._ai_products_by_display = {}
        for row in self.database.product_sync_rows():
            sku = str(row.get("sku") or "").strip()
            if not sku:
                continue
            display = " | ".join(filter(None, (
                str(row.get("offer_id") or "").strip(), sku,
                str(row.get("product_name") or "").strip(),
            )))
            self._ai_products_by_display[display] = dict(row)
        values = (all_label, *self._ai_products_by_display.keys())
        self._ai_product_values = values
        self.ai_product_combo.configure(values=values)
        if previous in values or self._product_from_input(self._ai_products_by_display, previous):
            self.ai_product_var.set(previous)
        else:
            self.ai_product_var.set(all_label)
        self._update_ai_product_ad_status()

    def _selected_ai_product(self) -> dict[str, Any] | None:
        return self._product_from_input(
            self._ai_products_by_display, self.ai_product_var.get()
        )

    def _update_ai_product_ad_status(self) -> None:
        if not hasattr(self, "ai_product_ad_status_var"):
            return
        product = self._selected_ai_product()
        if not product:
            self.ai_product_ad_status_var.set(
                "全店模式：只读取数据中心本地缓存，不会请求 Ozon"
            )
            return
        try:
            date_from, date_to = self._date_range()
            detail = self.database.product_ai_detail(
                date_from, date_to, str(product.get("sku") or "")
            )
            counts = detail.get("row_counts", {})
            exact_count = int(counts.get("advertising_exact_sku") or 0)
            campaign_rows = int(counts.get("advertising_matched_campaign") or 0)
            campaign_count = int(counts.get("advertising_matched_campaigns") or 0)
        except Exception:
            exact_count = campaign_rows = campaign_count = 0
        if exact_count:
            self.ai_product_ad_status_var.set(
                f"本地精确 SKU 广告缓存 {exact_count} 条；AI 页面不会请求 Ozon"
            )
        elif campaign_rows:
            self.ai_product_ad_status_var.set(
                f"从数据中心缓存匹配 {campaign_count} 个专属活动、{campaign_rows} 条日数据；"
                "AI 将标注为活动级推算，不会请求 Ozon"
            )
        else:
            self.ai_product_ad_status_var.set(
                "本地缓存未找到可可靠归因的专属活动；仍发送全店广告效率，"
                "不会从 AI 页面请求 Ozon"
            )

    def _selected_inventory_product(self) -> dict[str, Any] | None:
        return self._product_from_input(
            self._inventory_products_by_display, self.inventory_product_var.get()
        )

    @staticmethod
    def _cost_family_prefix(offer_id: str) -> str:
        """Return the editable family token before colour/size suffixes."""
        text = str(offer_id or "").strip()
        return re.split(r"[-_\s]+", text, maxsplit=1)[0] if text else ""

    @staticmethod
    def _product_from_input(
        mapping: dict[str, dict[str, Any]], value: str,
    ) -> dict[str, Any] | None:
        text = str(value or "").strip()
        if not text:
            return None
        if text in mapping:
            return mapping[text]
        normalized = text.casefold()
        exact = [
            row for row in mapping.values()
            if normalized in {
                str(row.get("sku") or "").strip().casefold(),
                str(row.get("offer_id") or "").strip().casefold(),
            }
        ]
        if len(exact) == 1:
            return exact[0]
        matches = [row for display, row in mapping.items() if normalized in display.casefold()]
        return matches[0] if len(matches) == 1 else None

    def _filter_combo_values(
        self, combo: ttk.Combobox, full_values: tuple[str, ...], query: str,
        *, open_dropdown: bool = True,
    ) -> tuple[str, ...]:
        text = str(query or "").strip().casefold()
        filtered = (
            full_values if not text
            else tuple(value for value in full_values if text in value.casefold())
        )
        combo.configure(values=filtered)
        if text and filtered and open_dropdown:
            self.after_idle(lambda: self._post_combo_dropdown(combo))
        return filtered

    def _post_combo_dropdown(self, combo: ttk.Combobox) -> None:
        """Open a filtered editable combobox without replacing the typed query."""
        try:
            if combo.winfo_exists() and combo.focus_get() == combo:
                self.tk.call("ttk::combobox::Post", str(combo))
        except tk.TclError:
            pass

    def _filter_combo_keyrelease(
        self,
        event: tk.Event[Any],
        combo: ttk.Combobox,
        full_values: tuple[str, ...],
        variable: tk.StringVar,
    ) -> None:
        if getattr(event, "keysym", "") in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        self._filter_combo_values(combo, full_values, variable.get())

    def _filter_inventory_products(self, event: tk.Event[Any]) -> None:
        if getattr(event, "keysym", "") in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        self.refresh_inventory_product_options()

    def _filter_ai_products(self, event: tk.Event[Any]) -> None:
        if getattr(event, "keysym", "") in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        self._filter_combo_values(
            self.ai_product_combo, getattr(self, "_ai_product_values", ()),
            self.ai_product_var.get(),
        )

    def _inventory_target_days(self) -> int:
        try:
            value = int(self.inventory_target_days_var.get().strip())
        except ValueError as error:
            raise ValueError("目标可售天数必须是整数。") from error
        if not 1 <= value <= 365:
            raise ValueError("目标可售天数应在 1–365 天之间。")
        return value

    def refresh_inventory_plan(self) -> None:
        window = getattr(self, "inventory_detail_window", None)
        tree = getattr(self, "inventory_tree", None)
        if window is None or not window.winfo_exists() or tree is None or not tree.winfo_exists():
            return
        product = self._selected_inventory_product()
        if not product:
            self.inventory_status_var.set("暂无商品；请先同步 Seller 商品目录。")
            return
        try:
            target_days = self._inventory_target_days()
        except ValueError as error:
            messagebox.showwarning("目标天数错误", str(error), parent=self)
            return
        sku = str(product.get("sku") or "")
        rows = self.database.inventory_cluster_plan(sku, target_days)
        self._clear_tree(self.inventory_tree)
        self._inventory_rows_by_iid = {}
        for index, row in enumerate(rows):
            macro_id = str(row.get("macrolocal_cluster_id") or "")
            iid = f"cluster-{macro_id or index}"
            self._inventory_rows_by_iid[iid] = row
            days = row.get("sellable_days_after")
            self.inventory_tree.insert("", "end", iid=iid, values=(
                row.get("cluster_name") or "—",
                row.get("cluster_name_cn") or "—",
                macro_id or "—",
                _fmt_int(row.get("available_stock") or 0),
                _fmt_int(row.get("transit_stock") or 0),
                _fmt_int(row.get("requested_stock") or 0),
                f"{float(row.get('daily_sales') or 0):.2f}",
                _fmt_int(round(float(row.get("monthly_estimated") or 0))),
                _fmt_int(row.get("idc_cluster") or 0),
                _fmt_int(row.get("recommended_qty") or 0),
                _fmt_int(row.get("planned_qty") or 0),
                f"{float(days):.1f}" if days is not None else "—",
            ), tags=("odd",) if index % 2 else ())
        available = sum(int(row.get("available_stock") or 0) for row in rows)
        transit = sum(int(row.get("transit_stock") or 0) for row in rows)
        daily = sum(float(row.get("daily_sales") or 0) for row in rows)
        recommended = sum(int(row.get("recommended_qty") or 0) for row in rows)
        planned = sum(int(row.get("planned_qty") or 0) for row in rows)
        values = {
            "available": available, "transit": transit, "daily": daily,
            "monthly": daily * 30, "recommended": recommended, "planned": planned,
        }
        for key, label in self.inventory_summary_labels.items():
            value = values[key]
            label.configure(text=f"{value:.2f}" if key == "daily" else _fmt_int(round(value)))
        stock_rows = sum(1 for row in rows if (
            row.get("available_stock") or row.get("transit_stock")
            or row.get("requested_stock") or row.get("daily_sales")
        ))
        self.inventory_status_var.set(
            f"已展示 {len(rows)} 个集群，其中 {stock_rows} 个有该 SKU 库存/销量数据。"
            if rows else "尚无集群库存快照，请点击“同步所选库存”。"
        )

    def sync_selected_inventory(self) -> None:
        product = self._selected_inventory_product()
        if not product:
            messagebox.showwarning("请选择商品", "请先选择要读取库存的商品。", parent=self)
            return
        credentials = self.service.load_credentials()
        if not credentials.seller_client_id or not credentials.seller_api_key:
            messagebox.showwarning(
                "缺少 Seller 凭证", "请先在 API 设置中保存 Seller API 凭证。", parent=self,
            )
            self.show_page("settings")
            return
        sku = str(product.get("sku") or "")

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.sync_inventory(
                sku, progress, self._cancel_event, event_listener
            )

        self._run_task("正在读取所选商品的集群库存…", action, self._inventory_sync_finished)

    def sync_all_inventory(self) -> None:
        credentials = self.service.load_credentials()
        if not credentials.seller_client_id or not credentials.seller_api_key:
            messagebox.showwarning(
                "缺少 Seller 凭证", "请先在 API 设置中保存 Seller API 凭证。", parent=self,
            )
            self.show_page("settings")
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.sync_inventory(
                None, progress, self._cancel_event, event_listener
            )

        self._run_task("正在分批读取全部商品库存…", action, self._inventory_sync_finished)

    def _inventory_sync_finished(self, result: dict[str, Any]) -> None:
        self.refresh_inventory_product_options()
        window = getattr(self, "inventory_detail_window", None)
        if window is not None and window.winfo_exists():
            self.refresh_inventory_plan()
        self.refresh_logs(preferred_log_id=result.get("log_id"))
        self.status_var.set(
            f"库存已更新：{result.get('stock_rows', 0)} 行，"
            f"{result.get('clusters', 0)} 个集群，{result.get('warehouses', 0)} 个发货地址"
        )

    def _inventory_row_selected(self, _event: tk.Event[Any] | None = None) -> None:
        selected = self.inventory_tree.selection()
        row = self._inventory_rows_by_iid.get(str(selected[0])) if selected else None
        if not row:
            return
        self.inventory_planned_qty_var.set(str(int(row.get("planned_qty") or 0)))
        self.inventory_selected_cluster_label.configure(
            text=(
                f"{row.get('cluster_name')} / {row.get('cluster_name_cn')}"
                f"（{row.get('macrolocal_cluster_id')}）"
            )
        )

    def _inventory_tree_double_click(self, event: tk.Event[Any]) -> None:
        """Place a real Entry over the editable replenishment-plan cell."""
        tree = self.inventory_tree
        if tree.identify_region(event.x, event.y) != "cell":
            return
        iid = tree.identify_row(event.y)
        column = tree.identify_column(event.x)
        # Column 11 is 计划补货; every other inventory value is read-only.
        if not iid or column != "#11" or iid not in self._inventory_rows_by_iid:
            return
        tree.selection_set(iid)
        tree.focus(iid)
        self._inventory_row_selected()
        bounds = tree.bbox(iid, column)
        if not bounds:
            return
        old_editor = getattr(self, "_inventory_inline_editor", None)
        if old_editor is not None and old_editor.winfo_exists():
            old_editor.destroy()
        x, y, width, height = bounds
        value = tk.StringVar(value=self.inventory_planned_qty_var.get())
        editor = ttk.Entry(tree, textvariable=value, justify="right")
        self._inventory_inline_editor = editor
        editor.place(x=x, y=y, width=width, height=height)
        editor.focus_set()
        editor.selection_range(0, "end")
        committed = {"done": False}

        def finish(save: bool) -> None:
            if committed["done"]:
                return
            committed["done"] = True
            text = value.get()
            if editor.winfo_exists():
                editor.destroy()
            if save:
                self.inventory_planned_qty_var.set(text)
                self.save_inventory_cluster_qty()

        editor.bind("<Return>", lambda _event: finish(True))
        editor.bind("<Escape>", lambda _event: finish(False))
        editor.bind("<FocusOut>", lambda _event: finish(True))

    def save_inventory_cluster_qty(self) -> None:
        product = self._selected_inventory_product()
        selected = self.inventory_tree.selection()
        row = self._inventory_rows_by_iid.get(str(selected[0])) if selected else None
        if not product or not row:
            messagebox.showwarning("请选择集群", "请先在表格中选择一个集群。", parent=self)
            return
        try:
            quantity = int(self.inventory_planned_qty_var.get().strip())
            target_days = self._inventory_target_days()
            if quantity < 0:
                raise ValueError
        except ValueError:
            messagebox.showwarning("数量错误", "计划补货量必须是大于等于 0 的整数。", parent=self)
            return
        self.database.save_replenishment_plan(
            str(product.get("sku") or ""),
            str(row.get("macrolocal_cluster_id") or ""), quantity, target_days,
        )
        self.refresh_inventory_plan()
        self.refresh_inventory_product_options()
        self.status_var.set("集群计划补货量已保存到本地")

    def apply_all_inventory_recommendations(self) -> None:
        product = self._selected_inventory_product()
        if not product:
            messagebox.showwarning("请选择商品", "请先选择要设置补货计划的商品。", parent=self)
            return
        rows = list(self._inventory_rows_by_iid.values())
        if not rows:
            messagebox.showwarning("暂无库存计划", "请先同步所选商品的库存。", parent=self)
            return
        try:
            target_days = self._inventory_target_days()
        except ValueError as error:
            messagebox.showwarning("目标天数错误", str(error), parent=self)
            return
        sku = str(product.get("sku") or "")
        for row in rows:
            self.database.save_replenishment_plan(
                sku,
                str(row.get("macrolocal_cluster_id") or ""),
                int(row.get("recommended_qty") or 0),
                target_days,
            )
        positive = sum(1 for row in rows if int(row.get("recommended_qty") or 0) > 0)
        total = sum(int(row.get("recommended_qty") or 0) for row in rows)
        self.refresh_inventory_plan()
        self.refresh_inventory_product_options()
        self.status_var.set(
            f"已保存全部建议：{positive} 个待补货集群，共 {total} 件；约仓计划可直接读取。"
        )

    def open_supply_for_inventory_product(self) -> None:
        product = self._selected_inventory_product()
        if not product:
            return
        sku = str(product.get("sku") or "")
        self.refresh_supply_product_options()
        display = next(
            (
                value for value, row in self._supply_products_by_display.items()
                if str(row.get("sku") or "") == sku
            ),
            "",
        )
        if display:
            self.supply_plan_product_var.set(display)
        self._close_inventory_product_detail()
        self.show_page("supply")
        if display:
            self.supply_plan_product_var.set(display)
        self.refresh_supply_plan()

    def refresh_supply_product_options(self) -> None:
        if not hasattr(self, "supply_plan_product_combo"):
            return
        previous = self.supply_plan_product_var.get()
        self._supply_products_by_display = {}
        for row in self.database.product_sync_rows():
            sku = str(row.get("sku") or "").strip()
            if not sku:
                continue
            display = " | ".join(filter(None, (
                str(row.get("offer_id") or "").strip(), sku,
                str(row.get("product_name") or "").strip(),
            )))
            self._supply_products_by_display[display] = dict(row)
        values = tuple(self._supply_products_by_display)
        self._supply_product_values = values
        self.supply_plan_product_combo.configure(values=values)
        if previous in self._supply_products_by_display or self._product_from_input(
            self._supply_products_by_display, previous
        ):
            self.supply_plan_product_var.set(previous)
        else:
            self.supply_plan_product_var.set(values[0] if values else "")
        self._refresh_supply_warehouse_options()
        self.refresh_supply_plan()

    def _filter_supply_products(self, event: tk.Event[Any]) -> None:
        if getattr(event, "keysym", "") in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        self._filter_combo_values(
            self.supply_plan_product_combo,
            getattr(self, "_supply_product_values", ()),
            self.supply_plan_product_var.get(),
        )

    def _selected_supply_product(self) -> dict[str, Any] | None:
        return self._product_from_input(
            self._supply_products_by_display, self.supply_plan_product_var.get()
        )

    def refresh_supply_plan(self) -> None:
        if not hasattr(self, "supply_plan_tree"):
            return
        product = self._selected_supply_product()
        self._clear_tree(self.supply_plan_tree)
        self._supply_plan_rows = []
        self._supply_plan_rows_by_iid = {}
        if not product:
            self.supply_plan_status_var.set(
                "请输入准确的货号/SKU，或从过滤后的列表中选择商品。"
            )
            return
        rows = self.database.inventory_cluster_plan(str(product.get("sku") or ""), 30)
        self._supply_plan_rows = [
            row for row in rows
            if row.get("plan_saved") and int(row.get("planned_qty") or 0) > 0
        ]
        for index, row in enumerate(self._supply_plan_rows):
            days = row.get("sellable_days_after")
            iid = f"supply-cluster-{row.get('macrolocal_cluster_id') or index}"
            self._supply_plan_rows_by_iid[iid] = row
            self.supply_plan_tree.insert("", "end", iid=iid, values=(
                row.get("cluster_name") or "—",
                row.get("cluster_name_cn") or "—",
                row.get("macrolocal_cluster_id") or "—",
                _fmt_int(row.get("available_stock") or 0),
                _fmt_int(row.get("transit_stock") or 0),
                _fmt_int(row.get("requested_stock") or 0),
                f"{float(row.get('daily_sales') or 0):.2f}",
                _fmt_int(row.get("planned_qty") or 0),
                f"{float(days):.1f}" if days is not None else "—",
            ), tags=("odd",) if index % 2 else ())
        quantity = sum(int(row.get("planned_qty") or 0) for row in self._supply_plan_rows)
        available = sum(int(row.get("available_stock") or 0) for row in self._supply_plan_rows)
        daily = sum(float(row.get("daily_sales") or 0) for row in self._supply_plan_rows)
        after_days = (
            sum(
                int(row.get("available_stock") or 0)
                + int(row.get("transit_stock") or 0)
                + int(row.get("requested_stock") or 0)
                + int(row.get("planned_qty") or 0)
                for row in self._supply_plan_rows
            ) / daily if daily > 0 else None
        )
        self.supply_plan_summary_labels["clusters"].configure(
            text=_fmt_int(len(self._supply_plan_rows))
        )
        self.supply_plan_summary_labels["quantity"].configure(text=_fmt_int(quantity))
        self.supply_plan_summary_labels["available"].configure(text=_fmt_int(available))
        self.supply_plan_summary_labels["after_days"].configure(
            text=f"{after_days:.1f}" if after_days is not None else "—"
        )
        self.supply_plan_status_var.set(
            f"已读取 {len(self._supply_plan_rows)} 个待补货集群，共 {quantity} 件；尚未提交 Ozon。"
            if self._supply_plan_rows else
            "该商品暂无已保存的计划补货数量，请先到“库存管理”双击数量或采用全部建议。"
        )

    def _refresh_supply_warehouse_options(self) -> None:
        if not hasattr(self, "supply_plan_warehouse_combo"):
            return
        previous = self.supply_plan_warehouse_var.get()
        self._supply_warehouses_by_display = {}
        for row in self.database.supply_warehouse_options():
            display = " | ".join(filter(None, (
                str(row.get("city") or ""), str(row.get("warehouse_name") or ""),
                str(row.get("seller_warehouse_id") or ""),
            )))
            self._supply_warehouses_by_display[display] = row
        values = tuple(self._supply_warehouses_by_display)
        self._supply_warehouse_values = values
        self.supply_plan_warehouse_combo.configure(values=values)
        self.supply_plan_warehouse_var.set(
            previous if previous in values else (values[0] if values else "")
        )
        self._supply_warehouse_selected()

    def _filter_supply_warehouses(self, event: tk.Event[Any]) -> None:
        if getattr(event, "keysym", "") in {"Up", "Down", "Return", "Escape", "Tab"}:
            return
        self._filter_combo_values(
            self.supply_plan_warehouse_combo,
            getattr(self, "_supply_warehouse_values", ()),
            self.supply_plan_warehouse_var.get(),
        )

    def _selected_supply_warehouse(self) -> dict[str, Any] | None:
        text = self.supply_plan_warehouse_var.get().strip()
        if text in self._supply_warehouses_by_display:
            return self._supply_warehouses_by_display[text]
        normalized = text.casefold()
        matches = [
            row for display, row in self._supply_warehouses_by_display.items()
            if normalized and normalized in display.casefold()
        ]
        return matches[0] if len(matches) == 1 else None

    def _supply_warehouse_selected(self, _event: tk.Event[Any] | None = None) -> None:
        row = self._selected_supply_warehouse()
        if not row:
            self.supply_plan_address_label.configure(
                text="请输入准确的仓库名称/城市，或从过滤列表中选择地址。"
            )
            return
        address = "，".join(filter(None, (
            str(row.get("region") or ""), str(row.get("city") or ""),
            str(row.get("address") or ""),
        )))
        status = "启用" if row.get("is_active") else "停用"
        self.supply_plan_address_label.configure(
            text=f"{address or '地址未返回'} · {row.get('timezone') or '时区未知'} · {status}"
        )

    def export_inventory_plan(self) -> None:
        product = self._selected_inventory_product()
        if not product:
            messagebox.showwarning("请选择商品", "请先选择商品。", parent=self)
            return
        try:
            target_days = self._inventory_target_days()
        except ValueError as error:
            messagebox.showwarning("目标天数错误", str(error), parent=self)
            return
        rows = list(self._inventory_rows_by_iid.values())
        if not rows:
            messagebox.showwarning("暂无计划", "请先同步并生成集群补货计划。", parent=self)
            return
        offer = re.sub(r"[^\w.-]+", "_", str(product.get("offer_id") or product.get("sku") or "product"))
        path = filedialog.asksaveasfilename(
            title="导出补货计划", defaultextension=".xlsx",
            initialfile=f"Ozon补货计划_{offer}_{date.today().isoformat()}.xlsx",
            filetypes=[("Excel 工作簿", "*.xlsx")], parent=self,
        )
        if not path:
            return
        try:
            export_replenishment_xlsx(
                Path(path), rows, product, target_days, "", None,
            )
        except ExportError as error:
            messagebox.showerror("导出失败", str(error), parent=self)
            return
        self.status_var.set(f"补货计划已导出：{path}")
        messagebox.showinfo("导出完成", f"已保存：\n{path}", parent=self)

    def preview_supply_draft(self) -> None:
        product = self._selected_supply_product()
        rows = list(self._supply_plan_rows)
        if self.supply_plan_type_var.get().strip() not in {"直送", "越库"}:
            messagebox.showwarning(
                "供应方式错误", "供应方式请输入“直送”或“越库”。", parent=self,
            )
            return
        if not product or not rows:
            messagebox.showwarning(
                "暂无补货数量", "请先选择商品，并为至少一个集群填写计划补货量。", parent=self,
            )
            return
        warehouse = self._selected_supply_warehouse() or {}
        total = sum(int(row.get("planned_qty") or 0) for row in rows)
        preview = {
            "status": "LOCAL_PREVIEW_NOT_SUBMITTED",
            "supply_type": self.supply_plan_type_var.get(),
            "product": {"sku": product.get("sku"), "offer_id": product.get("offer_id")},
            "seller_warehouse": {
                "id": warehouse.get("seller_warehouse_id"),
                "name": warehouse.get("warehouse_name"), "address": warehouse.get("address"),
            },
            "clusters": [
                {"macrolocal_cluster_id": row.get("macrolocal_cluster_id"),
                 "cluster_name": row.get("cluster_name"),
                 "cluster_name_cn": row.get("cluster_name_cn"),
                 "quantity": row.get("planned_qty")}
                for row in rows
            ],
            "total_quantity": total,
        }
        self.supply_plan_status_var.set(
            f"本地草稿已生成：{len(rows)} 个集群，共 {total} 件；尚未提交 Ozon。"
        )
        messagebox.showinfo(
            "本地约仓草稿（未提交）",
            json.dumps(preview, ensure_ascii=False, indent=2), parent=self,
        )

    def open_supply_api_wizard(self, event: tk.Event[Any] | None = None) -> None:
        if event is not None and getattr(event, "y", None) is not None:
            iid = self.supply_plan_tree.identify_row(event.y)
            if iid:
                self.supply_plan_tree.selection_set(iid)
                self.supply_plan_tree.focus(iid)
        product = self._selected_supply_product()
        selected = self.supply_plan_tree.selection()
        if selected:
            cluster = self._supply_plan_rows_by_iid.get(str(selected[0]))
        else:
            cluster = self._supply_plan_rows[0] if self._supply_plan_rows else None
            if cluster:
                iid = next(iter(self._supply_plan_rows_by_iid), "")
                if iid:
                    self.supply_plan_tree.selection_set(iid)
        if not product or not cluster:
            messagebox.showwarning(
                "请选择待约仓集群",
                "请先选择商品，并在待约仓集群表中选择一行。计划补货量必须大于 0。",
                parent=self,
            )
            return
        if not str(product.get("sku") or "").isdigit():
            messagebox.showwarning("SKU 无效", "Ozon SKU 必须是数字。", parent=self)
            return
        credentials = self.service.load_credentials()
        if not credentials.seller_client_id or not credentials.seller_api_key:
            messagebox.showwarning(
                "缺少 Seller 凭证", "请先在 API 设置中保存 Seller API 凭证。", parent=self,
            )
            self.show_page("settings")
            return

        old_window = getattr(self, "supply_api_window", None)
        if old_window is not None and old_window.winfo_exists():
            old_window.destroy()
        window = tk.Toplevel(self)
        self.supply_api_window = window
        window.title("创建新的 Ozon 约仓")
        window.geometry("980x650")
        window.minsize(860, 590)
        window.configure(bg=PALETTE.bg)
        window.transient(self)

        self._supply_api_product = dict(product)
        self._supply_api_cluster = dict(cluster)
        self._supply_api_type = (
            "CROSSDOCK" if self.supply_plan_type_var.get().strip() == "越库" else "DIRECT"
        )
        self._supply_api_draft_id = 0
        self._supply_api_dropoffs_by_display: dict[str, dict[str, Any]] = {}
        self._supply_api_storage_by_display: dict[str, dict[str, Any]] = {}
        self._supply_api_slots_by_display: dict[str, dict[str, Any]] = {}
        self.supply_api_search_var = tk.StringVar()
        self.supply_api_dropoff_var = tk.StringVar()
        self.supply_api_storage_var = tk.StringVar()
        self.supply_api_date_from_var = tk.StringVar(
            value=(date.today() + timedelta(days=1)).isoformat()
        )
        self.supply_api_date_to_var = tk.StringVar(
            value=(date.today() + timedelta(days=15)).isoformat()
        )
        self.supply_api_slot_var = tk.StringVar()
        self.supply_api_status_var = tk.StringVar(
            value="尚未创建远程草稿。每个写操作只发送一次，并在发送前要求确认。"
        )

        header = tk.Frame(window, bg=PALETTE.bg)
        header.pack(fill="x", padx=24, pady=(20, 12))
        tk.Label(
            header, text="创建新的 Ozon 约仓",
            font=("Microsoft YaHei UI", 19, "bold"), fg=PALETTE.text, bg=PALETTE.bg,
        ).pack(anchor="w")
        tk.Label(
            header,
            text=(
                f"{product.get('offer_id') or product.get('sku')} · SKU {product.get('sku')} · "
                f"{cluster.get('cluster_name_cn') or cluster.get('cluster_name')} "
                f"({cluster.get('macrolocal_cluster_id')}) · "
                f"计划 {int(cluster.get('planned_qty') or 0)} 件 · "
                f"{'越库' if self._supply_api_type == 'CROSSDOCK' else '直送'}"
            ),
            font=("Microsoft YaHei UI", 9), fg=PALETTE.muted, bg=PALETTE.bg,
        ).pack(anchor="w", pady=(3, 0))

        step1 = self._panel(window)
        step1.pack(fill="x", padx=24, pady=(0, 8))
        self._panel_title(
            step1, "1. 发货方式与发货点",
            "越库必须搜索并选择发货点；直送会由草稿计算目的仓，可跳过搜索。",
        )
        line1 = tk.Frame(step1, bg="#FFFFFF")
        line1.pack(fill="x", padx=16, pady=(0, 10))
        tk.Label(line1, text="发货点搜索", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        ttk.Entry(line1, width=25, textvariable=self.supply_api_search_var).pack(
            side="left", padx=(6, 6)
        )
        self._button(
            line1, "搜索缓存", self.search_supply_api_dropoffs, secondary=True, compact=True,
        ).pack(side="left", padx=(0, 8))
        self._button(
            line1, "联网刷新", lambda: self.search_supply_api_dropoffs(force_refresh=True),
            secondary=True, compact=True,
        ).pack(side="left", padx=(0, 8))
        self.supply_api_dropoff_combo = ttk.Combobox(
            line1, state="normal", width=54, textvariable=self.supply_api_dropoff_var,
        )
        self.supply_api_dropoff_combo.pack(side="left", fill="x", expand=True)
        self.supply_api_dropoff_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.supply_api_dropoff_combo,
                getattr(self, "_supply_api_dropoff_values", ()),
                self.supply_api_dropoff_var,
            ),
        )

        step2 = self._panel(window)
        step2.pack(fill="x", padx=24, pady=(0, 8))
        self._panel_title(
            step2, "2. 创建并校验草稿", "创建草稿是 Ozon 写操作；随后只读校验并选择目的仓。",
        )
        line2 = tk.Frame(step2, bg="#FFFFFF")
        line2.pack(fill="x", padx=16, pady=(0, 10))
        self._button(
            line2, "创建 Ozon 草稿", self.create_supply_api_draft, compact=True,
        ).pack(side="left")
        self._button(
            line2, "刷新校验结果", self.refresh_supply_api_draft,
            secondary=True, compact=True,
        ).pack(side="left", padx=6)
        tk.Label(line2, text="目的仓", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left", padx=(14, 0))
        self.supply_api_storage_combo = ttk.Combobox(
            line2, state="normal", width=55, textvariable=self.supply_api_storage_var,
        )
        self.supply_api_storage_combo.pack(side="left", padx=(6, 0), fill="x", expand=True)
        self.supply_api_storage_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.supply_api_storage_combo,
                getattr(self, "_supply_api_storage_values", ()),
                self.supply_api_storage_var,
            ),
        )

        step3 = self._panel(window)
        step3.pack(fill="x", padx=24, pady=(0, 8))
        self._panel_title(step3, "3. 查询可用时段", "日期和时间均使用 Ozon 返回的仓库当地时区。")
        line3 = tk.Frame(step3, bg="#FFFFFF")
        line3.pack(fill="x", padx=16, pady=(0, 10))
        tk.Label(line3, text="日期", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        ttk.Entry(line3, width=12, textvariable=self.supply_api_date_from_var).pack(side="left", padx=(6, 4))
        tk.Label(line3, text="至", bg="#FFFFFF", fg=PALETTE.muted).pack(side="left")
        ttk.Entry(line3, width=12, textvariable=self.supply_api_date_to_var).pack(side="left", padx=(4, 6))
        self._button(
            line3, "查询时段", self.query_supply_api_timeslots,
            secondary=True, compact=True,
        ).pack(side="left", padx=(0, 8))
        self.supply_api_slot_combo = ttk.Combobox(
            line3, state="normal", width=48, textvariable=self.supply_api_slot_var,
        )
        self.supply_api_slot_combo.pack(side="left", fill="x", expand=True)
        self.supply_api_slot_combo.bind(
            "<KeyRelease>",
            lambda event: self._filter_combo_keyrelease(
                event, self.supply_api_slot_combo,
                getattr(self, "_supply_api_slot_values", ()),
                self.supply_api_slot_var,
            ),
        )

        step4 = self._panel(window)
        step4.pack(fill="both", expand=True, padx=24, pady=(0, 18))
        self._panel_title(
            step4, "4. 正式创建供应申请",
            "确认后将真实写入 Ozon；遇到超时不会自动重试，请先到 Ozon 后台核对。",
        )
        line4 = tk.Frame(step4, bg="#FFFFFF")
        line4.pack(fill="x", padx=16, pady=(0, 8))
        self._button(
            line4, "正式提交约仓申请", self.submit_supply_api_order, compact=True,
        ).pack(side="left")
        tk.Label(
            line4, textvariable=self.supply_api_status_var, wraplength=690,
            justify="left", anchor="w", bg="#FFFFFF", fg=PALETTE.muted,
            font=("Microsoft YaHei UI", 8),
        ).pack(side="left", padx=14, fill="x", expand=True)
        if self._supply_api_type == "CROSSDOCK":
            cached = self.database.cached_supply_dropoffs("", self._supply_api_type)
            if cached:
                self._supply_api_dropoffs_loaded({
                    "warehouses": cached, "cached": True, "log_id": None,
                })

    def _supply_api_window_exists(self) -> bool:
        window = getattr(self, "supply_api_window", None)
        return bool(window is not None and window.winfo_exists())

    @staticmethod
    def _value_from_mapping(
        mapping: dict[str, dict[str, Any]], value: str,
    ) -> dict[str, Any] | None:
        text = str(value or "").strip()
        if text in mapping:
            return mapping[text]
        normalized = text.casefold()
        matches = [row for display, row in mapping.items() if normalized in display.casefold()]
        return matches[0] if normalized and len(matches) == 1 else None

    def search_supply_api_dropoffs(self, force_refresh: bool = False) -> None:
        if self._supply_api_type == "DIRECT":
            self.supply_api_status_var.set("直送草稿无需手动指定发货点，可直接创建草稿。")
            return
        query = self.supply_api_search_var.get().strip()
        if len(query) < 4:
            messagebox.showwarning(
                "搜索词过短", "请至少输入 4 个字符搜索发货点。", parent=self.supply_api_window,
            )
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.search_supply_dropoffs(
                query, self._supply_api_type, progress,
                self._cancel_event, event_listener, force_refresh=force_refresh,
            )

        status = "正在从 Ozon 刷新发货点…" if force_refresh else "正在搜索本店发货点缓存…"
        self._run_task(status, action, self._supply_api_dropoffs_loaded)

    def _supply_api_dropoffs_loaded(self, result: dict[str, Any]) -> None:
        if not self._supply_api_window_exists():
            return
        self._supply_api_dropoffs_by_display = {}
        for row in result.get("warehouses") or []:
            display = " | ".join(filter(None, (
                str(row.get("name") or ""), str(row.get("address") or ""),
                str(row.get("warehouse_type") or ""), str(row.get("warehouse_id") or ""),
            )))
            self._supply_api_dropoffs_by_display[display] = dict(row)
        values = tuple(self._supply_api_dropoffs_by_display)
        self._supply_api_dropoff_values = values
        self.supply_api_dropoff_combo.configure(values=values)
        self.supply_api_dropoff_var.set(values[0] if values else "")
        source = "本店缓存" if result.get("cached") else "Ozon API（已写入本店缓存）"
        self.supply_api_status_var.set(f"从{source}找到 {len(values)} 个可用发货点。")
        if result.get("log_id"):
            self.refresh_logs(preferred_log_id=result.get("log_id"))

    def create_supply_api_draft(self) -> None:
        product = self._supply_api_product
        cluster = self._supply_api_cluster
        dropoff = None
        delivery_info = None
        if self._supply_api_type == "CROSSDOCK":
            dropoff = self._value_from_mapping(
                self._supply_api_dropoffs_by_display, self.supply_api_dropoff_var.get()
            )
            if not dropoff:
                messagebox.showwarning(
                    "请选择发货点", "越库草稿必须先搜索并选择一个准确的发货点。",
                    parent=self.supply_api_window,
                )
                return
            delivery_info = {
                "type": "DROPOFF",
                "drop_off_warehouse": {
                    "warehouse_id": int(dropoff.get("warehouse_id") or 0),
                    "warehouse_type": "CROSS_DOCK",
                },
            }
        confirmed = messagebox.askyesno(
            "确认创建远程草稿",
            (
                "这会在 Ozon 创建一个新的约仓草稿，但还不会正式创建供应单。\n\n"
                f"SKU：{product.get('sku')}\n"
                f"集群：{cluster.get('cluster_name_cn') or cluster.get('cluster_name')} "
                f"({cluster.get('macrolocal_cluster_id')})\n"
                f"数量：{int(cluster.get('planned_qty') or 0)} 件\n"
                f"方式：{'越库' if self._supply_api_type == 'CROSSDOCK' else '直送'}\n"
                f"发货点：{(dropoff or {}).get('name') or '由 Ozon 草稿计算'}\n\n"
                "该写操作只发送一次。确定继续吗？"
            ),
            icon="warning", parent=self.supply_api_window,
        )
        if not confirmed:
            return

        def action() -> dict[str, Any]:
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.create_supply_draft(
                str(product.get("sku") or ""), int(cluster.get("planned_qty") or 0),
                int(cluster.get("macrolocal_cluster_id") or 0), self._supply_api_type,
                delivery_info, event_listener,
            )

        self._run_task("正在创建 Ozon 约仓草稿（仅一次）…", action, self._supply_api_draft_created)

    def _supply_api_draft_created(self, result: dict[str, Any]) -> None:
        if not self._supply_api_window_exists():
            return
        self._supply_api_draft_id = int(result.get("draft_id") or 0)
        self._apply_supply_api_draft_info(result.get("info") or {})
        self.refresh_logs(preferred_log_id=result.get("log_id"))

    def refresh_supply_api_draft(self) -> None:
        if not self._supply_api_draft_id:
            messagebox.showwarning(
                "尚无草稿", "请先创建 Ozon 草稿。", parent=self.supply_api_window,
            )
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.supply_draft_info(
                self._supply_api_draft_id, progress, self._cancel_event, event_listener,
            )

        self._run_task("正在刷新草稿校验结果…", action, self._supply_api_draft_info_loaded)

    def _supply_api_draft_info_loaded(self, result: dict[str, Any]) -> None:
        if not self._supply_api_window_exists():
            return
        self._apply_supply_api_draft_info(result.get("info") or {})
        self.refresh_logs(preferred_log_id=result.get("log_id"))

    def _apply_supply_api_draft_info(self, info: dict[str, Any]) -> None:
        cluster_id = str(self._supply_api_cluster.get("macrolocal_cluster_id") or "")
        self._supply_api_storage_by_display = {}
        for cluster in info.get("clusters") or []:
            if str(cluster.get("macrolocal_cluster_id") or "") != cluster_id:
                continue
            for row in cluster.get("warehouses") or []:
                storage = row.get("storage_warehouse") or {}
                warehouse_id = int(storage.get("warehouse_id") or 0)
                if not warehouse_id:
                    continue
                availability = row.get("availability_status") or {}
                display = " | ".join(filter(None, (
                    str(storage.get("name") or ""), str(storage.get("address") or ""),
                    str(availability.get("state") or ""), str(warehouse_id),
                )))
                self._supply_api_storage_by_display[display] = {
                    **dict(row), "storage_warehouse_id": warehouse_id,
                }
        values = tuple(self._supply_api_storage_by_display)
        self._supply_api_storage_values = values
        self.supply_api_storage_combo.configure(values=values)
        self.supply_api_storage_var.set(values[0] if values else "")
        status = str(info.get("status") or "处理中")
        errors = info.get("errors") or []
        self.supply_api_status_var.set(
            f"草稿 {self._supply_api_draft_id} · 状态 {status} · 可选目的仓 {len(values)} 个"
            + (f" · Ozon 返回错误 {len(errors)} 项" if errors else "")
            + ("。若仍在计算，请稍后点击“刷新校验结果”。" if not values else "。")
        )

    def query_supply_api_timeslots(self) -> None:
        storage = self._value_from_mapping(
            self._supply_api_storage_by_display, self.supply_api_storage_var.get()
        )
        if not self._supply_api_draft_id or not storage:
            messagebox.showwarning(
                "草稿尚未就绪", "请先创建草稿、等待校验完成并选择目的仓。",
                parent=self.supply_api_window,
            )
            return
        try:
            date.fromisoformat(self.supply_api_date_from_var.get().strip())
            date.fromisoformat(self.supply_api_date_to_var.get().strip())
        except ValueError:
            messagebox.showwarning(
                "日期格式错误", "日期必须使用 YYYY-MM-DD 格式。", parent=self.supply_api_window,
            )
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.supply_draft_timeslots(
                self._supply_api_draft_id, self._supply_api_type,
                int(self._supply_api_cluster.get("macrolocal_cluster_id") or 0),
                int(storage.get("storage_warehouse_id") or 0),
                self.supply_api_date_from_var.get().strip(),
                self.supply_api_date_to_var.get().strip(),
                progress, self._cancel_event, event_listener,
            )

        self._run_task("正在查询草稿可用约仓时段…", action, self._supply_api_timeslots_loaded)

    def _supply_api_timeslots_loaded(self, result: dict[str, Any]) -> None:
        if not self._supply_api_window_exists():
            return
        self._supply_api_slots_by_display = {}
        for row in result.get("timeslots") or []:
            start_at = str(row.get("from") or "")
            end_at = str(row.get("to") or "")
            display = (
                f"{row.get('day') or start_at[:10]} | {start_at[11:16]}–{end_at[11:16]}"
                f" | {row.get('timezone') or '仓库当地时区'}"
            )
            self._supply_api_slots_by_display[display] = dict(row)
        values = tuple(self._supply_api_slots_by_display)
        self._supply_api_slot_values = values
        self.supply_api_slot_combo.configure(values=values)
        self.supply_api_slot_var.set(values[0] if values else "")
        self.supply_api_status_var.set(
            f"草稿 {self._supply_api_draft_id} 已返回 {len(values)} 个可用时段。"
        )
        self.refresh_logs(preferred_log_id=result.get("log_id"))

    def submit_supply_api_order(self) -> None:
        storage = self._value_from_mapping(
            self._supply_api_storage_by_display, self.supply_api_storage_var.get()
        )
        slot = self._value_from_mapping(
            self._supply_api_slots_by_display, self.supply_api_slot_var.get()
        )
        if not self._supply_api_draft_id or not storage or not slot:
            messagebox.showwarning(
                "信息不完整", "请先完成草稿校验、选择目的仓并查询选择时段。",
                parent=self.supply_api_window,
            )
            return
        confirmed = messagebox.askyesno(
            "最终确认：创建真实供应申请",
            (
                "此操作会真实创建 Ozon 供应申请。\n\n"
                f"草稿 ID：{self._supply_api_draft_id}\n"
                f"SKU：{self._supply_api_product.get('sku')}\n"
                f"集群：{self._supply_api_cluster.get('cluster_name_cn') or self._supply_api_cluster.get('cluster_name')}\n"
                f"数量：{int(self._supply_api_cluster.get('planned_qty') or 0)} 件\n"
                f"目的仓：{self.supply_api_storage_var.get()}\n"
                f"时段：{self.supply_api_slot_var.get()}\n\n"
                "提交只执行一次，超时后请先到 Ozon 后台核对，不能直接重复提交。确定继续吗？"
            ),
            icon="warning", parent=self.supply_api_window,
        )
        if not confirmed:
            return

        def action() -> dict[str, Any]:
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.submit_supply_from_draft(
                self._supply_api_draft_id, self._supply_api_type,
                int(self._supply_api_cluster.get("macrolocal_cluster_id") or 0),
                int(storage.get("storage_warehouse_id") or 0),
                str(slot.get("from") or ""), str(slot.get("to") or ""),
                event_listener,
            )

        self._run_task("正在正式提交约仓申请（仅一次）…", action, self._supply_api_order_submitted)

    def _supply_api_order_submitted(self, result: dict[str, Any]) -> None:
        if not self._supply_api_window_exists():
            return
        status = result.get("status") or {}
        order_id = status.get("order_id")
        state = status.get("status") or "已受理，生成中"
        self.supply_api_status_var.set(
            f"Ozon 已受理 · 状态 {state} · 供应单 {order_id or '生成中'}"
        )
        self.refresh_logs(preferred_log_id=result.get("log_id"))
        messagebox.showinfo(
            "Ozon 已受理",
            (
                f"约仓申请已提交。\n状态：{state}\n供应单 ID：{order_id or '仍在生成'}\n\n"
                "请到 Ozon 后台或活动供应单中核对最终结果。"
            ),
            parent=self.supply_api_window,
        )

    def refresh_supply_orders(self) -> None:
        credentials = self.service.load_credentials()
        if not credentials.seller_client_id or not credentials.seller_api_key:
            messagebox.showwarning(
                "缺少 Seller 凭证", "请先在 API 设置中保存 Seller API 凭证。", parent=self,
            )
            self.show_page("settings")
            return

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.supply_appointments(
                progress, self._cancel_event, event_listener
            )

        self._run_task("正在读取 FBO 供应单与越库信息…", action, self._supply_orders_loaded)

    def _supply_orders_loaded(self, result: dict[str, Any]) -> None:
        rows = list(result.get("orders") or [])
        self._clear_tree(self.supply_order_tree)
        self._clear_tree(self.supply_timeslot_tree)
        self._supply_orders_by_id = {}
        self._supply_timeslots_by_iid = {}
        for index, row in enumerate(rows):
            order_id = str(row.get("order_id") or "")
            if not order_id:
                continue
            self._supply_orders_by_id[order_id] = row
            local_from = str(row.get("local_timeslot_from") or "")
            local_to = str(row.get("local_timeslot_to") or "")
            current_slot = "未预约"
            if local_from and local_to:
                current_slot = f"{local_from[:10]} {local_from[11:16]}–{local_to[11:16]}"
            self.supply_order_tree.insert("", "end", iid=order_id, values=(
                row.get("order_number") or order_id,
                _supply_state_label(str(row.get("state") or "")),
                row.get("supply_type") or "—",
                int(row.get("crossdock_supplies") or 0),
                row.get("clusters") or "—",
                row.get("dropoff_name") or "—",
                current_slot,
                int(row.get("supplies_count") or 0),
            ), tags=("odd",) if index % 2 else ())
        crossdock_count = sum(1 for row in rows if row.get("is_crossdock"))
        self.supply_status_var.set(
            f"已读取 {len(rows)} 个活动供应单，其中 {crossdock_count} 个包含越库批次。"
        )
        self.supply_slot_hint.configure(text="请选择一个供应单，再查询 Ozon 返回的可用时间窗。")
        self.refresh_logs(preferred_log_id=result.get("log_id"))

    def _supply_order_selected(self, _event: tk.Event[Any] | None = None) -> None:
        row = self._selected_supply_order()
        if not row:
            return
        timezone_name = row.get("timezone_name") or "仓库当地时区"
        cluster_text = row.get("clusters") or "非越库"
        self.supply_slot_hint.configure(
            text=(
                f"已选 {row.get('order_number') or row.get('order_id')} · "
                f"{row.get('supply_type')} · 集群 {cluster_text} · {timezone_name}。"
            )
        )

    def _selected_supply_order(self) -> dict[str, Any] | None:
        selected = self.supply_order_tree.selection()
        if not selected:
            return None
        return self._supply_orders_by_id.get(str(selected[0]))

    def query_supply_timeslots(self, auto_select: bool = False) -> None:
        order = self._selected_supply_order()
        if not order:
            messagebox.showwarning("请选择供应单", "请先在活动供应单表中选择一行。", parent=self)
            return
        try:
            start = date.fromisoformat(self.supply_date_from_var.get().strip())
            end = date.fromisoformat(self.supply_date_to_var.get().strip())
            if start > end:
                raise ValueError("开始日期不能晚于结束日期。")
            start_minutes = _clock_minutes(self.supply_hour_from_var.get(), allow_24=False)
            end_minutes = _clock_minutes(self.supply_hour_to_var.get(), allow_24=True)
            if start_minutes >= end_minutes:
                raise ValueError("当地时段的结束时间必须晚于开始时间。")
        except ValueError as error:
            messagebox.showwarning("筛选条件错误", str(error), parent=self)
            return
        order_id = int(order["order_id"])
        timezone_offset = int(order.get("timezone_offset_seconds") or 0)

        def action() -> dict[str, Any]:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.supply_timeslots(
                order_id, start.isoformat(), end.isoformat(), timezone_offset,
                progress, self._cancel_event, event_listener,
            )

        callback = lambda result: self._supply_timeslots_loaded(
            result, order_id, start_minutes, end_minutes, auto_select
        )
        self._run_task("正在查询可用约仓时间窗…", action, callback)

    def auto_select_earliest_timeslot(self) -> None:
        self.query_supply_timeslots(auto_select=True)

    def _supply_timeslots_loaded(
        self, result: dict[str, Any], order_id: int,
        start_minutes: int, end_minutes: int, auto_select: bool,
    ) -> None:
        current = self._selected_supply_order()
        if not current or int(current.get("order_id") or 0) != order_id:
            return
        all_rows = list(result.get("timeslots") or [])
        rows = []
        for row in all_rows:
            local_time = str(row.get("local_from") or "")[11:16]
            try:
                minutes = _clock_minutes(local_time, allow_24=False)
            except ValueError:
                continue
            if start_minutes <= minutes < end_minutes:
                rows.append(row)
        self._clear_tree(self.supply_timeslot_tree)
        self._supply_timeslots_by_iid = {}
        timezone_name = current.get("timezone_name") or "仓库当地时区"
        for index, row in enumerate(rows):
            iid = f"slot-{index}"
            self._supply_timeslots_by_iid[iid] = row
            self.supply_timeslot_tree.insert("", "end", iid=iid, values=(
                row.get("local_day") or "—", row.get("local_time") or "—",
                timezone_name, row.get("from") or "—", row.get("to") or "—",
            ), tags=("odd",) if index % 2 else ())
        self.supply_slot_hint.configure(
            text=(
                f"Ozon 返回 {len(all_rows)} 个时间窗；按当地时段筛选后剩余 {len(rows)} 个。"
                "提交时使用 API 返回的原始时间，避免时区偏移。"
            )
        )
        self.refresh_logs(preferred_log_id=result.get("log_id"))
        if auto_select:
            if not rows:
                messagebox.showinfo(
                    "没有匹配时间窗", "所选日期和当地时段内没有可用时间窗。", parent=self,
                )
                return
            first = self.supply_timeslot_tree.get_children()[0]
            self.supply_timeslot_tree.selection_set(first)
            self.supply_timeslot_tree.focus(first)
            self.supply_timeslot_tree.see(first)
            self.book_selected_timeslot()

    def book_selected_timeslot(self) -> None:
        order = self._selected_supply_order()
        selected = self.supply_timeslot_tree.selection()
        if not order or not selected:
            messagebox.showwarning(
                "请选择时间窗", "请先选择供应单并在下方选择一个可用时间窗。", parent=self,
            )
            return
        slot = self._supply_timeslots_by_iid.get(str(selected[0]))
        if not slot:
            return
        cluster_text = order.get("clusters") or "非越库"
        local_text = f"{slot.get('local_day')} {slot.get('local_time')}"
        confirmed = messagebox.askyesno(
            "确认提交约仓",
            (
                "此操作会真实修改 Ozon 供应单的预约时间。\n\n"
                f"供应单：{order.get('order_number') or order.get('order_id')}\n"
                f"类型：{order.get('supply_type')}\n"
                f"越库集群：{cluster_text}\n"
                f"仓库当地时间：{local_text}\n"
                f"API 原始时间：{slot.get('from')} 至 {slot.get('to')}\n\n"
                "写操作只提交一次，不会自动重试。确定继续吗？"
            ),
            icon="warning", parent=self,
        )
        if not confirmed:
            self.status_var.set("已取消约仓提交，Ozon 数据未修改")
            return
        order_id = int(order["order_id"])

        def action() -> dict[str, Any]:
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            return self.service.book_supply_timeslot(
                order_id, str(slot["from"]), str(slot["to"]), event_listener
            )

        callback = lambda result: self._supply_booking_submitted(result, local_text)
        self._run_task("正在提交约仓请求（仅一次）…", action, callback)

    def _supply_booking_submitted(self, result: dict[str, Any], local_text: str) -> None:
        operation_id = str(result.get("operation_id") or "")
        self.refresh_logs(preferred_log_id=result.get("log_id"))
        self.supply_status_var.set(f"约仓请求已提交：{local_text}")
        detail = f"\n操作 ID：{operation_id}" if operation_id else ""
        messagebox.showinfo(
            "Ozon 已受理",
            (
                f"约仓请求已提交，目标时间为 {local_text}。{detail}\n\n"
                "请点击“读取活动供应单”核对最终状态；系统不会对写操作自动重试。"
            ),
            parent=self,
        )

    def refresh_logs(self, preferred_log_id: int | None = None) -> None:
        if not hasattr(self, "log_tree"):
            return
        selected = self.log_tree.selection()
        selected_id = str(preferred_log_id) if preferred_log_id is not None else (
            selected[0] if selected else ""
        )
        rows = self.database.recent_logs()
        self._clear_tree(self.log_tree)
        for index, row in enumerate(rows):
            status = {
                "success": "成功", "failed": "失败", "running": "进行中",
                "cached": "使用缓存",
            }.get(row["status"], row["status"])
            self.log_tree.insert("", "end", iid=str(row["id"]), values=(
                str(row["started_at"]).replace("T", " "), row["source"], status,
                row["rows_count"], row["message"],
            ), tags=("odd",) if index % 2 else ())
        children = self.log_tree.get_children()
        if selected_id and selected_id in children:
            self.log_tree.selection_set(selected_id)
            self.log_tree.see(selected_id)
            self.refresh_log_details(int(selected_id))
        elif children:
            self.log_tree.selection_set(children[0])
            self.refresh_log_details(int(children[0]))

    def _log_selected(self, _event: tk.Event[Any] | None = None) -> None:
        selected = self.log_tree.selection()
        if selected:
            self.refresh_log_details(int(selected[0]))

    def refresh_log_details(self, log_id: int) -> None:
        if not hasattr(self, "log_detail_text"):
            return
        events = self.database.log_events(log_id)
        self.log_detail_title.configure(text=f"运行明细 · 任务 #{log_id} · {len(events)} 条")
        lines: list[str] = []
        for item in events:
            event_at = str(item["event_at"]).replace("T", " ")
            details = item.get("details") or {}
            line = f"{event_at}  [{item['level']}] [{item['stage']}] {item['message']}"
            if details:
                line += "\n    " + json.dumps(details, ensure_ascii=False, sort_keys=True)
            lines.append(line)
        if not lines:
            lines.append("该任务由旧版本创建，没有保存逐步运行明细。")
        self.log_detail_text.configure(state="normal")
        self.log_detail_text.delete("1.0", "end")
        self.log_detail_text.insert("1.0", "\n".join(lines))
        self.log_detail_text.configure(state="disabled")
        self.log_detail_text.see("end")

    def copy_log_details(self) -> None:
        if not hasattr(self, "log_detail_text"):
            return
        text = self.log_detail_text.get("1.0", "end-1c")
        if not text:
            return
        self.clipboard_clear()
        self.clipboard_append(text)
        self.status_var.set("运行明细已复制")

    def _on_log_event(
        self, log_id: int, _level: str, _stage: str,
        _message: str, _details: dict[str, Any],
    ) -> None:
        self.refresh_logs(preferred_log_id=log_id)

    @staticmethod
    def _clear_tree(tree: ttk.Treeview) -> None:
        children = tree.get_children()
        if children:
            tree.delete(*children)

    def _credentials_from_form(self) -> Credentials:
        return Credentials(**{key: variable.get().strip() for key, variable in self.credential_vars.items()})

    def export_api_profiles(self) -> None:
        if not messagebox.askyesno(
            "导出 API 配置",
            "迁移文件会包含 Seller、Performance、AI 和飞书的明文密钥。\n\n"
            "请只保存到可信位置，并在目标电脑导入后删除文件。是否继续？",
            parent=self,
        ):
            return
        path = filedialog.asksaveasfilename(
            parent=self, title="导出全部店铺 API 配置",
            defaultextension=".ozon-api.json",
            filetypes=[("Ozon API 配置包", "*.ozon-api.json"), ("JSON", "*.json")],
            initialfile=f"ozon-api-configs-{date.today().isoformat()}.ozon-api.json",
        )
        if not path:
            return
        try:
            rows = []
            for profile in self.shop_registry.list():
                database = Database(self.shop_registry.database_path(profile))
                service = AppService(database, shop_name=profile.name, shop_kind=profile.kind)
                rows.append((profile, service.load_credentials()))
            write_api_bundle(Path(path), build_api_bundle(rows))
            self.status_var.set(f"已导出 {len(rows)} 个命名 API 配置")
            messagebox.showinfo(
                "导出完成",
                f"已导出 {len(rows)} 个店铺配置。\n请妥善保管并在迁移完成后删除该明文文件。",
                parent=self,
            )
        except Exception as error:
            messagebox.showerror("导出失败", str(error), parent=self)

    def import_api_profiles(self) -> None:
        path = filedialog.askopenfilename(
            parent=self, title="导入 API 配置",
            filetypes=[("Ozon API 配置包", "*.ozon-api.json *.json"), ("所有文件", "*.*")],
        )
        if not path:
            return
        try:
            items = read_api_bundle(Path(path))
            if not messagebox.askyesno(
                "确认导入",
                f"文件包含 {len(items)} 个店铺 API。\n"
                "同名 API 会更新现有配置；不存在的会新建独立店铺数据库。是否继续？",
                parent=self,
            ):
                return
            existing = {
                profile.resolved_api_name.casefold(): profile
                for profile in self.shop_registry.list()
            }
            imported: list[ShopProfile] = []
            created = updated = 0
            for item in items:
                key = item["api_name"].casefold()
                profile = existing.get(key)
                if profile is None:
                    profile = self.shop_registry.add(
                        item["shop_name"], item["shop_kind"], api_name=item["api_name"]
                    )
                    existing[key] = profile
                    created += 1
                else:
                    profile = self.shop_registry.update(
                        profile.id, name=item["shop_name"], kind=item["shop_kind"],
                        api_name=item["api_name"],
                    )
                    updated += 1
                database = Database(self.shop_registry.database_path(profile))
                AppService(
                    database, shop_name=profile.name, shop_kind=profile.kind,
                ).save_credentials(item["credentials"])
                imported.append(profile)
            self._refresh_shop_selector()
            if imported:
                self.switch_shop(imported[0].id, force_reload=True)
                self.show_page("settings")
            self.status_var.set(f"API 导入完成：新增 {created}，更新 {updated}")
            messagebox.showinfo(
                "导入完成",
                f"新增 {created} 个配置，更新 {updated} 个配置。\n"
                "密钥已在本机重新加密保存。",
                parent=self,
            )
        except (ApiBundleError, ValueError, KeyError, OSError) as error:
            messagebox.showerror("导入失败", str(error), parent=self)

    def save_credentials(self) -> None:
        try:
            self.service.save_credentials(self._credentials_from_form())
            self.status_var.set(f"{self.active_shop.display_name} 的独立 API 设置已保存")
            messagebox.showinfo(
                "保存成功",
                f"凭证已加密保存到：{self.active_shop.display_name}\n"
                "其他店铺的 API 配置和数据均未改变。",
                parent=self,
            )
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)

    def test_seller(self) -> None:
        self._run_task("正在测试 Seller API…", lambda: self.service.test_seller(self._credentials_from_form()), self._show_test_result)

    def test_performance(self) -> None:
        self._run_task("正在测试 Performance API…", lambda: self.service.test_performance(self._credentials_from_form()), self._show_test_result)

    def test_ai(self) -> None:
        credentials = self._credentials_from_form()
        if not credentials.ai_api_key:
            messagebox.showwarning("缺少 AI 密钥", "请先填写 AI 中转站 API Key。", parent=self)
            return
        self._run_task(
            "正在测试 AI 中转站…",
            lambda: self.service.test_ai(credentials),
            self._show_test_result,
        )

    def save_feishu_credentials(self) -> None:
        try:
            self.service.save_credentials(self._credentials_from_form())
            self.feishu_status_var.set("飞书配置已加密保存")
            self.status_var.set("飞书设置已保存")
            messagebox.showinfo(
                "保存成功", "飞书应用配置已加密保存在本机。", parent=self,
            )
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)

    def test_feishu(self) -> None:
        credentials = self._credentials_from_form()
        if not credentials.feishu_app_id or not credentials.feishu_app_secret:
            messagebox.showwarning(
                "缺少飞书凭证", "请先填写飞书 App ID 和 App Secret。", parent=self,
            )
            return
        try:
            self.service.save_credentials(credentials)
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)
            return

        def finished(message: str) -> None:
            self.feishu_status_var.set(message)
            self._show_test_result(message)

        self._run_task(
            "正在测试飞书连接…", lambda: self.service.test_feishu(credentials), finished,
        )

    def start_feishu_product_sync(self, direction: str) -> None:
        credentials = self._credentials_from_form()
        try:
            self.service.save_credentials(credentials)
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)
            return
        event_listener = lambda *event: self.after(0, self._on_log_event, *event)

        def finished(result: dict[str, Any]) -> None:
            self.refresh_logs(preferred_log_id=result.get("log_id"))
            message = (
                f"读取 {result['pulled']} 条，新增 {result['created']} 条，"
                f"更新 {result['updated']} 条，未变化 {result['unchanged']} 条。"
            )
            if result.get("duplicates"):
                message += f" 飞书表中发现 {result['duplicates']} 个重复 SKU，已跳过重复项。"
            if result.get("partial"):
                message += f" {result['partial']} 条成本不完整，已保留本地已有值。"
            self.feishu_status_var.set("商品同步完成：" + message)
            self.refresh_profit()
            messagebox.showinfo("飞书商品同步完成", message, parent=self)

        self._run_task(
            "正在同步飞书商品多维表格…",
            lambda: self.service.sync_feishu_products(
                direction, credentials, event_listener
            ),
            finished,
        )

    def start_feishu_weekly_sync(
        self,
        write_table: bool,
        send_message: bool,
        *,
        week_end: date | None = None,
        automatic: bool = False,
    ) -> None:
        credentials = self.service.load_credentials() if automatic else self._credentials_from_form()
        if not automatic:
            try:
                self.service.save_credentials(credentials)
            except Exception as error:
                messagebox.showerror("保存失败", str(error), parent=self)
                return
        if week_end is None:
            try:
                week_end = date.fromisoformat(self.weekly_end_var.get().strip())
            except ValueError:
                messagebox.showwarning(
                    "日期错误", "周报结束日格式应为 YYYY-MM-DD。", parent=self,
                )
                return
            week_end -= timedelta(days=(week_end.weekday() + 1) % 7)
        event_listener = lambda *event: self.after(0, self._on_log_event, *event)

        def finished(result: dict[str, Any]) -> None:
            self.refresh_logs(preferred_log_id=result.get("log_id"))
            if automatic:
                self.database.save_settings({
                    "feishu_weekly_last_sent_end": week_end.isoformat(),
                })
                status = f"{result['period']} 周报已自动写入飞书并发送到群"
                self.status_var.set(status)
                self.feishu_status_var.set(status)
                return
            parts = []
            if write_table:
                parts.append(f"周报表已{result['table_action']}")
            if send_message:
                parts.append("群消息已发送")
            message = f"{result['period']}：" + "，".join(parts) + "。"
            self.feishu_status_var.set(message)
            messagebox.showinfo("飞书周报完成", message, parent=self)

        self._run_task(
            "正在同步飞书周报…",
            lambda: self.service.sync_feishu_weekly(
                week_end.isoformat(), write_table=write_table, send_message=send_message,
                credentials=credentials, event_listener=event_listener,
            ),
            finished,
        )

    def _toggle_feishu_weekly_auto(self) -> None:
        enabled = self.feishu_weekly_auto_var.get()
        self.database.save_settings({
            "feishu_weekly_auto_enabled": "1" if enabled else "0",
        })
        self.feishu_status_var.set(
            "每周自动写表并发群已开启" if enabled else "每周自动写表并发群已关闭"
        )

    def start_feishu_monthly_profit(self) -> None:
        credentials = self._credentials_from_form()
        month = self.feishu_month_var.get().strip()
        try:
            date.fromisoformat(month + "-01")
            self.service.save_credentials(credentials)
        except ValueError:
            messagebox.showwarning(
                "月份错误", "核算月份格式应为 YYYY-MM。", parent=self,
            )
            return
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)
            return
        event_listener = lambda *event: self.after(0, self._on_log_event, *event)

        def finished(result: dict[str, Any]) -> None:
            self.refresh_logs(preferred_log_id=result.get("log_id"))
            message = f"{self.active_shop.name} · {result['period']} 月度盈亏表已发送到群。"
            self.feishu_status_var.set(message)
            messagebox.showinfo("飞书月度盈亏表", message, parent=self)

        self._run_task(
            "正在生成并发送月度盈亏卡片…",
            lambda: self.service.send_feishu_monthly_profit(
                month, credentials=credentials, event_listener=event_listener,
            ),
            finished,
        )

    def start_feishu_cross_border_profit(self) -> None:
        if self.active_shop.kind != "cross_border":
            messagebox.showwarning(
                "请切换跨境店",
                "跨境周利润只能读取跨境店独立数据库，请先切换到跨境 API 配置。",
                parent=self,
            )
            return
        credentials = self._credentials_from_form()
        date_from = self.feishu_cross_from_var.get().strip()
        date_to = self.feishu_cross_to_var.get().strip()
        try:
            if date.fromisoformat(date_from) > date.fromisoformat(date_to):
                raise ValueError("开始日期不能晚于结束日期。")
            self.service.save_credentials(credentials)
        except ValueError as error:
            messagebox.showwarning(
                "日期错误", str(error) or "日期格式应为 YYYY-MM-DD。", parent=self,
            )
            return
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)
            return
        event_listener = lambda *event: self.after(0, self._on_log_event, *event)

        def finished(result: dict[str, Any]) -> None:
            self.refresh_logs(preferred_log_id=result.get("log_id"))
            message = f"{self.active_shop.name} · {result['period']} 跨境周利润表已发送到群。"
            self.feishu_status_var.set(message)
            messagebox.showinfo("飞书跨境周利润表", message, parent=self)

        self._run_task(
            "正在生成并发送跨境周利润卡片…",
            lambda: self.service.send_feishu_cross_border_profit(
                date_from, date_to, credentials=credentials,
                event_listener=event_listener,
            ),
            finished,
        )

    def install_feishu_schedule(self) -> None:
        credentials = self._credentials_from_form()
        required = all((
            credentials.seller_client_id, credentials.seller_api_key,
            credentials.performance_client_id, credentials.performance_client_secret,
            credentials.feishu_app_id, credentials.feishu_app_secret,
            credentials.feishu_app_token, credentials.feishu_weekly_table_id,
            credentials.feishu_chat_id,
        ))
        if not required:
            messagebox.showwarning(
                "配置未完成",
                "安装后台任务前，请先完整填写 Seller、Performance、飞书应用、周报表和群 Chat ID。",
                parent=self,
            )
            return
        try:
            self.service.save_credentials(credentials)
            self.feishu_weekly_auto_var.set(True)
            self.database.save_settings({"feishu_weekly_auto_enabled": "1"})
            message = install_weekly_task(self.root_path)
            self.feishu_status_var.set(message)
            messagebox.showinfo(
                "计划任务已安装",
                message + "\n\n移动软件目录后请重新安装计划任务。",
                parent=self,
            )
        except Exception as error:
            messagebox.showerror("安装失败", str(error), parent=self)

    def remove_feishu_schedule(self) -> None:
        try:
            message = remove_weekly_task()
            self.feishu_status_var.set(message)
            messagebox.showinfo("计划任务", message, parent=self)
        except Exception as error:
            messagebox.showerror("移除失败", str(error), parent=self)

    def _maybe_start_auto_feishu_weekly(self, week_end: date) -> bool:
        if not self.feishu_weekly_auto_var.get() or self._task_running:
            return False
        if self.database.get_setting("feishu_weekly_last_sent_end") >= week_end.isoformat():
            return False
        week_start = week_end - timedelta(days=6)
        cache = self.database.sales_sync_state(
            week_start.isoformat(), week_end.isoformat()
        )
        if str(cache.get("verified_through") or "") < week_end.isoformat():
            self.feishu_status_var.set("自动周报等待 Seller 数据核验完整")
            return False
        credentials = self.service.load_credentials()
        complete = all((
            credentials.feishu_app_id,
            credentials.feishu_app_secret,
            credentials.feishu_app_token,
            credentials.feishu_weekly_table_id,
            credentials.feishu_chat_id,
        ))
        if not complete:
            self.feishu_status_var.set("自动周报等待完整飞书配置")
            return False
        self.start_feishu_weekly_sync(
            True, True, week_end=week_end, automatic=True,
        )
        return True

    def start_ai_analysis(self, analysis_type: str) -> None:
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        credentials = self._credentials_from_form()
        if not credentials.ai_api_key:
            messagebox.showwarning(
                "缺少 AI 密钥", "请先在 API 设置中填写并保存 AI 中转站 API Key。", parent=self,
            )
            self.show_page("settings")
            return
        question = self.ai_question_text.get("1.0", "end-1c").strip() if analysis_type == "custom" else ""
        if analysis_type == "custom" and not question:
            messagebox.showwarning("请输入问题", "请输入需要 AI 分析的问题。", parent=self)
            return
        type_labels = {
            "overview": "经营诊断", "advertising": "广告优化",
            "products": "商品销量", "custom": "自定义问题",
        }
        selected_product = self._selected_ai_product()
        if (
            not selected_product
            and self.ai_product_var.get().strip()
            and self.ai_product_var.get().strip() != "全部经营数据（商品/活动 Top 20）"
        ):
            messagebox.showwarning(
                "商品未唯一匹配",
                "请输入完整货号或 SKU，或从过滤后的下拉结果中选择一个商品。",
                parent=self,
            )
            return
        product_sku = str(selected_product.get("sku") or "") if selected_product else None
        scope_text = (
            f"SKU {product_sku} 本地缓存" if product_sku
            else "全店汇总与 Top 20 排名"
        )
        self.ai_result_hint.configure(
            text=(
                f"正在分析 {date_from} 至 {date_to} · "
                f"{type_labels.get(analysis_type, 'AI 分析')} · {scope_text}"
            )
        )

        def action() -> str:
            return self.service.ai_analysis(
                date_from, date_to, question, analysis_type,
                credentials=credentials,
                product_sku=product_sku,
            )

        self._run_task("正在调用 AI 中转站…", action, self._ai_analysis_finished)

    def _ai_analysis_finished(self, result: str) -> None:
        self.last_ai_result = str(result)
        self.refresh_logs()
        self._update_ai_product_ad_status()
        self.ai_output_text.configure(state="normal")
        self.ai_output_text.delete("1.0", "end")
        for line in self.last_ai_result.splitlines():
            self._insert_ai_markdown_line(line)
        self.ai_output_text.configure(state="disabled")
        self.ai_output_text.see("1.0")
        self.ai_result_hint.configure(text="分析完成")
        self.show_page("ai")

    def clear_ai_result(self) -> None:
        self.last_ai_result = ""
        self.ai_output_text.configure(state="normal")
        self.ai_output_text.delete("1.0", "end")
        self.ai_output_text.configure(state="disabled")
        self.ai_result_hint.configure(text="等待分析")

    def _insert_ai_markdown_line(self, line: str) -> None:
        heading = re.match(r"^(#{1,3})\s+(.*)$", line)
        if heading:
            level = len(heading.group(1))
            self.ai_output_text.insert("end", heading.group(2) + "\n", f"h{level}")
            return
        if line.strip() == "---":
            self.ai_output_text.insert("end", "─" * 58 + "\n", "h3")
            return
        position = 0
        for match in re.finditer(r"\*\*(.+?)\*\*", line):
            self.ai_output_text.insert("end", line[position:match.start()])
            self.ai_output_text.insert("end", match.group(1), "bold")
            position = match.end()
        self.ai_output_text.insert("end", line[position:] + "\n")

    def export_ai_as_word(self) -> None:
        self._export_ai_report("docx")

    def export_ai_as_pdf(self) -> None:
        self._export_ai_report("pdf")

    def _export_ai_report(self, file_type: str) -> None:
        if not self.last_ai_result.strip():
            messagebox.showwarning("没有分析结果", "请先完成一次 AI 分析。", parent=self)
            return
        try:
            date_from, date_to = self._date_range()
            model = self._credentials_from_form().ai_model or "OpenAI 兼容模型"
            if file_type == "docx":
                path = filedialog.asksaveasfilename(
                    parent=self, title="导出 AI 分析为 Word",
                    defaultextension=".docx", filetypes=[("Word 文档", "*.docx")],
                    initialfile=f"Ozon_AI经营分析_{date_from}_{date_to}.docx",
                )
                if not path:
                    return
                export_ai_docx(Path(path), self.last_ai_result, date_from, date_to, model)
            else:
                path = filedialog.asksaveasfilename(
                    parent=self, title="导出 AI 分析为 PDF",
                    defaultextension=".pdf", filetypes=[("PDF 文档", "*.pdf")],
                    initialfile=f"Ozon_AI经营分析_{date_from}_{date_to}.pdf",
                )
                if not path:
                    return
                export_ai_pdf(Path(path), self.last_ai_result, date_from, date_to, model)
            self.status_var.set(f"AI 分析报告已导出：{path}")
            messagebox.showinfo("导出完成", f"报告已保存到：\n{path}", parent=self)
        except (ExportError, OSError, ValueError) as error:
            messagebox.showerror("导出失败", str(error), parent=self)

    def _show_test_result(self, result: str) -> None:
        messagebox.showinfo("连接成功", result, parent=self)

    def start_sync(self, source: str) -> None:
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        credentials = self.service.load_credentials()
        if source in {"all", "seller"} and (not credentials.seller_client_id or not credentials.seller_api_key):
            messagebox.showwarning("缺少 Seller 凭证", "请先在 API 设置中保存 Seller API 凭证。", parent=self)
            self.show_page("settings")
            return
        if source in {"all", "performance"} and (not credentials.performance_client_id or not credentials.performance_client_secret):
            messagebox.showwarning("缺少广告凭证", "请先在 API 设置中保存 Performance API 凭证。", parent=self)
            self.show_page("settings")
            return

        def action() -> Any:
            progress = lambda text: self.after(0, self.status_var.set, text)
            event_listener = lambda *event: self.after(0, self._on_log_event, *event)
            if source == "seller":
                return self.service.sync_seller(
                    date_from, date_to, progress, self._cancel_event, event_listener
                )
            if source == "performance":
                return self.service.sync_performance(
                    date_from, date_to, progress, self._cancel_event, event_listener
                )
            return self.service.sync_all(
                date_from, date_to, progress, self._cancel_event, event_listener
            )

        self._run_task("正在同步 Ozon 数据…", action, self._sync_finished)

    def cancel_sync(self) -> None:
        if self._cancel_event is not None and self._task_running:
            self._cancel_event.set()
            self.status_var.set("正在取消同步…")
            self.cancel_button.configure(state="disabled")

    def _sync_finished(self, results: Any) -> None:
        self.refresh_all()
        message = "\n".join(f"{result.source}: {result.rows} 行，{result.message}" for result in results)
        has_warning = any(
            marker in result.message for result in results
            for marker in ("失败", "限频", "缓存")
        )
        if has_warning:
            messagebox.showwarning("同步完成（有提示）", message or "同步已完成。", parent=self)
        else:
            messagebox.showinfo("同步完成", message or "同步已完成。", parent=self)

    def seed_demo(self) -> None:
        def action() -> tuple[int, int]:
            return self.database.seed_demo_data()
        self._run_task("正在生成演示数据…", action, self._demo_finished)

    def _demo_finished(self, counts: tuple[int, int]) -> None:
        self.date_from_var.set((date.today() - timedelta(days=29)).isoformat())
        self.date_to_var.set(date.today().isoformat())
        self.refresh_all()
        messagebox.showinfo("演示数据已生成", f"销量数据 {counts[0]} 行，广告数据 {counts[1]} 行。", parent=self)

    def delete_demo(self) -> None:
        counts = self.database.demo_counts()
        total = counts["sales"] + counts["ads"] + counts["campaigns"]
        if total == 0:
            messagebox.showinfo("没有演示数据", "当前数据库中没有可删除的演示数据。", parent=self)
            return
        confirmed = messagebox.askyesno(
            "删除演示数据",
            "将只删除以下演示记录，真实 API 数据不会受到影响：\n\n"
            f"演示销量：{counts['sales']} 行\n"
            f"演示广告：{counts['ads']} 行\n"
            f"演示活动：{counts['campaigns']} 个\n\n"
            "确认删除吗？",
            parent=self,
        )
        if not confirmed:
            return
        removed = self.database.clear_demo_data()
        self.refresh_all()
        messagebox.showinfo(
            "删除完成",
            f"已删除演示销量 {removed['sales']} 行、演示广告 {removed['ads']} 行、"
            f"演示活动 {removed['campaigns']} 个。",
            parent=self,
        )

    def export_csv(self) -> None:
        try:
            date_from, date_to = self._date_range()
        except ValueError as error:
            messagebox.showwarning("日期错误", str(error), parent=self)
            return
        folder = filedialog.askdirectory(title="选择导出目录", parent=self)
        if not folder:
            return
        paths = self.database.export_csv_bundle(Path(folder), date_from, date_to)
        messagebox.showinfo("导出完成", "已导出：\n" + "\n".join(str(path) for path in paths), parent=self)

    def _run_task(self, status: str, action: Callable[[], Any], on_success: Callable[[Any], None]) -> None:
        if self._task_running:
            messagebox.showinfo("任务进行中", "请等待当前任务完成。", parent=self)
            return
        self._task_running = True
        self._cancel_event = threading.Event()
        if hasattr(self, "cancel_button"):
            self.cancel_button.configure(state="normal")
        self.status_var.set(status)
        self.progress.start(10)

        def worker() -> None:
            try:
                result = action()
            except Exception as error:
                self.after(0, self._task_failed, error)
            else:
                self.after(0, self._task_succeeded, result, on_success)

        threading.Thread(target=worker, daemon=True).start()

    def _task_succeeded(self, result: Any, callback: Callable[[Any], None]) -> None:
        self._task_running = False
        self._cancel_event = None
        if hasattr(self, "cancel_button"):
            self.cancel_button.configure(state="disabled")
        self.progress.stop()
        self.status_var.set("任务已完成")
        callback(result)

    def _task_failed(self, error: Exception) -> None:
        self._task_running = False
        self._cancel_event = None
        if hasattr(self, "cancel_button"):
            self.cancel_button.configure(state="disabled")
        self.progress.stop()
        self.status_var.set("任务失败")
        self.refresh_logs()
        messagebox.showerror("操作失败", str(error), parent=self)


def _clock_minutes(value: str, *, allow_24: bool) -> int:
    text = str(value or "").strip()
    if not re.fullmatch(r"\d{2}:\d{2}", text):
        raise ValueError("时间格式应为 HH:MM。")
    hour, minute = (int(part) for part in text.split(":"))
    if hour == 24 and minute == 0 and allow_24:
        return 24 * 60
    if hour not in range(24) or minute not in range(60):
        raise ValueError("请输入有效的时间。")
    return hour * 60 + minute


def _supply_state_label(value: str) -> str:
    labels = {
        "ORDER_STATE_DATA_FILLING": "待填写",
        "ORDER_STATE_READY_TO_SUPPLY": "待供货",
        "ORDER_STATE_ACCEPTED_AT_SUPPLY_WAREHOUSE": "供货仓已接收",
        "ORDER_STATE_IN_TRANSIT": "运输中",
        "ORDER_STATE_ACCEPTANCE_AT_STORAGE_WAREHOUSE": "存储仓验收",
        "ORDER_STATE_REPORTS_CONFIRMATION_AWAITING": "待确认报告",
        "ORDER_STATE_REPORT_REJECTED": "报告被拒绝",
        "ORDER_STATE_COMPLETED": "已完成",
        "ORDER_STATE_CANCELLED": "已取消",
        "ORDER_STATE_REJECTED_AT_SUPPLY_WAREHOUSE": "供货仓拒收",
        "ORDER_STATE_UNSPECIFIED": "未知",
    }
    return labels.get(value, value.replace("ORDER_STATE_", "") or "—")


def _fmt_money(value: float) -> str:
    return f"{float(value):,.0f}".replace(",", " ")


def _fmt_int(value: float) -> str:
    return f"{int(round(float(value))):,}".replace(",", " ")


def _fmt_decimal(value: float) -> str:
    text = f"{float(value):,.2f}".replace(",", " ")
    return text.rstrip("0").rstrip(".")


def _average(rows: list[dict[str, Any]], key: str) -> float:
    values = [float(row.get(key) or 0) for row in rows]
    return sum(values) / len(values) if values else 0.0


def _fmt_change(value: float | None) -> str:
    if value is None:
        return "—"
    return f"{float(value):+.0f}%"


def _latest_completed_week_end(reference: date | None = None) -> date:
    current = reference or date.today()
    return current - timedelta(days=current.weekday() + 1)


def _month_bounds(value: str) -> tuple[date, date]:
    text = value.strip()
    if not re.fullmatch(r"\d{4}-\d{2}", text):
        raise ValueError("核算月份格式应为 YYYY-MM。")
    try:
        start = date.fromisoformat(text + "-01")
    except ValueError as error:
        raise ValueError("请输入有效的核算月份。") from error
    if start.month == 12:
        next_month = date(start.year + 1, 1, 1)
    else:
        next_month = date(start.year, start.month + 1, 1)
    return start, next_month - timedelta(days=1)


def _short_number(value: float) -> str:
    if abs(value) >= 1_000_000:
        return f"{value / 1_000_000:.1f}M"
    if abs(value) >= 1_000:
        return f"{value / 1_000:.0f}K"
    return f"{value:.0f}"


def _optional_nonnegative_float(value: str, label: str) -> float | None:
    text = value.strip().replace(" ", "").replace(",", ".")
    if not text:
        return None
    try:
        result = float(text)
    except ValueError as error:
        raise ValueError(f"{label}必须是数字。") from error
    if result < 0:
        raise ValueError(f"{label}不能小于 0。")
    return result
