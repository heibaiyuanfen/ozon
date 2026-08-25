from __future__ import annotations

import threading
import tkinter as tk
from datetime import date, timedelta
from pathlib import Path
from tkinter import messagebox, ttk
from typing import Any, Callable

from .config import PALETTE
from .wb_service import WBService, previous_full_week


PURPLE = "#7C3AED"
PURPLE_DARK = "#6D28D9"
PURPLE_SOFT = "#F3E8FF"
TEXT = "#172033"
MUTED = "#667085"


class WBWorkspace(tk.Toplevel):
    """A complete WB workspace with its own token, cache and calculations."""

    def __init__(self, master: tk.Misc, data_dir: str | Path | None = None) -> None:
        super().__init__(master)
        root_path = Path(data_dir or getattr(master, "root_path", Path.cwd()))
        self.service = WBService(root_path / "data")
        self.title("WB 跨境独立工作台")
        self.geometry("1480x880")
        self.minsize(1120, 700)
        self.configure(bg=PALETTE.bg)
        self.protocol("WM_DELETE_WINDOW", self.withdraw)
        self._pages: dict[str, tk.Frame] = {}
        self._nav: dict[str, tk.Button] = {}
        self._busy = False
        self._daily_rows: list[dict[str, Any]] = []
        self._cost_rows: dict[str, dict[str, Any]] = {}
        self._build_styles()
        self._build_shell()
        self._build_daily_page()
        self._build_cost_page()
        self._build_weekly_page()
        self._build_settings_page()
        self._load_settings()
        self._show_page("daily")

    def _build_styles(self) -> None:
        style = ttk.Style(self)
        style.configure(
            "WB.Treeview", background="#FFFFFF", fieldbackground="#FFFFFF",
            foreground=TEXT, rowheight=34, borderwidth=0, font=("Microsoft YaHei UI", 9),
        )
        style.configure(
            "WB.Treeview.Heading", background="#F8F7FC", foreground="#475467",
            relief="flat", font=("Microsoft YaHei UI", 9, "bold"), padding=(8, 9),
        )
        style.map("WB.Treeview", background=[("selected", "#EDE9FE")], foreground=[("selected", TEXT)])

    def _build_shell(self) -> None:
        sidebar = tk.Frame(self, bg="#2E1065", width=225)
        sidebar.pack(side="left", fill="y")
        sidebar.pack_propagate(False)
        tk.Label(sidebar, text="WB", bg="#2E1065", fg="#FFFFFF", font=("Segoe UI", 30, "bold")).pack(
            anchor="w", padx=25, pady=(28, 0)
        )
        tk.Label(
            sidebar, text="CROSS-BORDER ANALYTICS", bg="#2E1065", fg="#D8B4FE",
            font=("Segoe UI", 8, "bold"),
        ).pack(anchor="w", padx=27, pady=(0, 26))
        items = [
            ("daily", "每日经营"), ("cost", "产品成本与运费"),
            ("weekly", "周利润与飞书"), ("settings", "WB API 设置"),
        ]
        for key, label in items:
            button = tk.Button(
                sidebar, text=label, anchor="w", relief="flat", bd=0,
                bg="#2E1065", fg="#F4EBFF", activebackground="#4C1D95",
                activeforeground="#FFFFFF", padx=27, pady=13,
                font=("Microsoft YaHei UI", 10), cursor="hand2",
                command=lambda name=key: self._show_page(name),
            )
            button.pack(fill="x")
            self._nav[key] = button
        tk.Label(
            sidebar, text="独立 Token · 独立数据库\n不会读取 Ozon 店铺数据", justify="left",
            bg="#2E1065", fg="#C4B5FD", font=("Microsoft YaHei UI", 8),
        ).pack(side="bottom", anchor="w", padx=25, pady=24)
        body = tk.Frame(self, bg=PALETTE.bg)
        body.pack(side="left", fill="both", expand=True)
        self.page_host = tk.Frame(body, bg=PALETTE.bg)
        self.page_host.pack(fill="both", expand=True)
        status_bar = tk.Frame(body, bg="#FFFFFF", height=34, highlightbackground=PALETTE.border, highlightthickness=1)
        status_bar.pack(fill="x", side="bottom")
        self.status_var = tk.StringVar(value="WB 本地工作台就绪")
        tk.Label(status_bar, textvariable=self.status_var, bg="#FFFFFF", fg=MUTED,
                 font=("Microsoft YaHei UI", 8)).pack(side="left", padx=16, pady=7)
        self.progress = ttk.Progressbar(status_bar, mode="indeterminate", length=125)
        self.progress.pack(side="right", padx=16, pady=8)

    def _new_page(self, key: str) -> tk.Frame:
        page = tk.Frame(self.page_host, bg=PALETTE.bg)
        page.place(relx=0, rely=0, relwidth=1, relheight=1)
        self._pages[key] = page
        return page

    def _show_page(self, key: str) -> None:
        self._pages[key].tkraise()
        for name, button in self._nav.items():
            button.configure(bg="#4C1D95" if name == key else "#2E1065")
        if key == "daily":
            self._refresh_daily()
        elif key == "cost":
            self._refresh_costs()
        elif key == "weekly":
            self._refresh_weekly()

    def _header(self, page: tk.Widget, title: str, subtitle: str) -> None:
        tk.Label(page, text=title, bg=PALETTE.bg, fg=TEXT,
                 font=("Microsoft YaHei UI", 24, "bold")).pack(anchor="w", padx=34, pady=(28, 2))
        tk.Label(page, text=subtitle, bg=PALETTE.bg, fg=MUTED,
                 font=("Microsoft YaHei UI", 9)).pack(anchor="w", padx=36, pady=(0, 18))

    def _panel(self, parent: tk.Widget) -> tk.Frame:
        frame = tk.Frame(parent, bg="#FFFFFF", highlightbackground=PALETTE.border, highlightthickness=1)
        return frame

    def _button(self, parent: tk.Widget, text: str, command: Callable[[], None], primary: bool = False) -> tk.Button:
        return tk.Button(
            parent, text=text, command=command, relief="flat", bd=0, cursor="hand2",
            bg=PURPLE if primary else "#F2F4F7", fg="#FFFFFF" if primary else TEXT,
            activebackground=PURPLE_DARK if primary else "#E4E7EC",
            activeforeground="#FFFFFF" if primary else TEXT,
            font=("Microsoft YaHei UI", 9, "bold"), padx=15, pady=8,
        )

    def _date_controls(self, parent: tk.Widget) -> tuple[tk.StringVar, tk.StringVar, tk.Frame]:
        end = date.today()
        start = end - timedelta(days=6)
        start_var, end_var = tk.StringVar(value=start.isoformat()), tk.StringVar(value=end.isoformat())
        frame = self._panel(parent)
        tk.Label(frame, text="日期", bg="#FFFFFF", fg=MUTED).pack(side="left", padx=(16, 8))
        ttk.Entry(frame, textvariable=start_var, width=12).pack(side="left", pady=12)
        tk.Label(frame, text="至", bg="#FFFFFF", fg=MUTED).pack(side="left", padx=8)
        ttk.Entry(frame, textvariable=end_var, width=12).pack(side="left", pady=12)
        return start_var, end_var, frame

    def _build_daily_page(self) -> None:
        page = self._new_page("daily")
        self._header(page, "WB 每日经营", "订单、精确商品广告、暂估平台费与利润；实时估算不等同 WB 最终财务结算")
        self.daily_from, self.daily_to, controls = self._date_controls(page)
        controls.pack(fill="x", padx=34, pady=(0, 14))
        self._button(controls, "刷新本地", self._refresh_daily).pack(side="left", padx=12)
        self._button(controls, "同步 WB API", self._sync_daily, True).pack(side="left")
        tk.Label(
            controls, text="广告按 日期 + nmId + 广告活动 ID 精确缓存", bg="#FFFFFF", fg="#7F56D9",
            font=("Microsoft YaHei UI", 8),
        ).pack(side="right", padx=16)
        cards = tk.Frame(page, bg=PALETTE.bg)
        cards.pack(fill="x", padx=34, pady=(0, 14))
        self.daily_cards: dict[str, tk.StringVar] = {}
        for key, label, color in (
            ("revenue", "销售额", PURPLE), ("quantity", "销量", "#2563EB"),
            ("ads", "商品广告费", "#F59E0B"), ("commission", "暂估平台费", "#EF4444"),
            ("profit", "暂估利润", "#10B981"),
        ):
            box = tk.Frame(cards, bg="#FFFFFF", highlightbackground=PALETTE.border, highlightthickness=1)
            box.pack(side="left", fill="x", expand=True, padx=(0, 10))
            tk.Frame(box, bg=color, height=3).pack(fill="x")
            tk.Label(box, text=label, bg="#FFFFFF", fg=MUTED, font=("Microsoft YaHei UI", 8)).pack(
                anchor="w", padx=16, pady=(12, 3))
            value = tk.StringVar(value="—")
            tk.Label(box, textvariable=value, bg="#FFFFFF", fg=TEXT,
                     font=("Segoe UI", 17, "bold")).pack(anchor="w", padx=16, pady=(0, 13))
            self.daily_cards[key] = value
        table_panel = self._panel(page)
        table_panel.pack(fill="both", expand=True, padx=34, pady=(0, 24))
        columns = ("day", "article", "nm", "qty", "revenue", "ad", "ad_orders", "tacos",
                   "warehouse", "mode", "logistics", "commission", "profit", "status")
        headings = ("日期", "货号", "nmId", "销量", "销售额 RUB", "广告费 RUB", "广告订单",
                    "TACOS", "发货仓", "仓库判定", "暂估物流 CNY", "暂估平台费 CNY", "暂估利润 CNY", "口径")
        widths = (95, 160, 95, 60, 105, 105, 80, 75, 130, 85, 105, 115, 115, 100)
        self.daily_tree = self._tree(table_panel, columns, headings, widths)

    def _build_cost_page(self) -> None:
        page = self._new_page("cost")
        self._header(page, "WB 产品成本与运费", "保存采购成本、尺寸重量和仓库归类；完整运费公式到位后只替换计算规则")
        form = self._panel(page)
        form.pack(fill="x", padx=34, pady=(0, 14))
        fields = [
            ("nm_id", "WB nmId", 12), ("article", "货号", 16), ("purchase", "采购 CNY", 10),
            ("length", "长 cm", 8), ("width", "宽 cm", 8), ("height", "高 cm", 8), ("weight", "重量 kg", 8),
        ]
        self.cost_vars = {name: tk.StringVar() for name, _, _ in fields}
        for name, label, width in fields:
            holder = tk.Frame(form, bg="#FFFFFF")
            holder.pack(side="left", padx=(14, 0), pady=12)
            tk.Label(holder, text=label, bg="#FFFFFF", fg=MUTED, font=("Microsoft YaHei UI", 8)).pack(anchor="w")
            ttk.Entry(holder, textvariable=self.cost_vars[name], width=width).pack(anchor="w", pady=(3, 0))
        mode_holder = tk.Frame(form, bg="#FFFFFF")
        mode_holder.pack(side="left", padx=14, pady=12)
        tk.Label(mode_holder, text="仓库/运费模式", bg="#FFFFFF", fg=MUTED,
                 font=("Microsoft YaHei UI", 8)).pack(anchor="w")
        self.cost_mode = tk.StringVar(value="auto")
        ttk.Combobox(mode_holder, textvariable=self.cost_mode, state="readonly", width=11,
                     values=("auto", "dongguan", "overseas")).pack(pady=(3, 0))
        self._button(form, "保存", self._save_cost, True).pack(side="left", padx=(0, 10), pady=(24, 12))
        self._button(form, "清空", self._clear_cost).pack(side="left", pady=(24, 12))
        tk.Label(
            page,
            text="暂行运费：东莞仓 ≤0.3kg 为 58×重量+2，否则 43×重量+8；海外仓为 8+max(体积L−1,0)×2。",
            bg=PALETTE.bg, fg="#B54708", font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=36, pady=(0, 10))
        panel = self._panel(page)
        panel.pack(fill="both", expand=True, padx=34, pady=(0, 24))
        columns = ("article", "nm", "purchase", "length", "width", "height", "weight", "mode")
        headings = ("货号", "nmId", "采购成本 CNY", "长 cm", "宽 cm", "高 cm", "重量 kg", "仓库模式")
        self.cost_tree = self._tree(panel, columns, headings, (210, 130, 120, 90, 90, 90, 90, 120))
        self.cost_tree.bind("<Double-1>", self._edit_cost)

    def _build_weekly_page(self) -> None:
        page = self._new_page("weekly")
        self._header(page, "WB 周利润与飞书", "手动预览并发送店铺周利润卡片；不会自动向群发送")
        start, end = previous_full_week()
        self.week_from, self.week_to, controls = self._date_controls(page)
        self.week_from.set(start)
        self.week_to.set(end)
        controls.pack(fill="x", padx=34, pady=(0, 14))
        self._button(controls, "预览周报", self._refresh_weekly).pack(side="left", padx=12)
        self._button(controls, "发送到飞书群", self._send_weekly, True).pack(side="left")
        self.week_panel = self._panel(page)
        self.week_panel.pack(fill="both", expand=True, padx=34, pady=(0, 24))

    def _build_settings_page(self) -> None:
        page = self._new_page("settings")
        self._header(page, "WB API 设置", "配置 WB 专用 Token、经营估算参数和飞书；密钥只加密保存在本机 WB 数据空间")
        area = tk.Frame(page, bg=PALETTE.bg)
        area.pack(fill="both", expand=True, padx=34, pady=(0, 24))
        api = self._panel(area)
        api.pack(side="left", fill="both", expand=True, padx=(0, 10))
        feishu = self._panel(area)
        feishu.pack(side="left", fill="both", expand=True)
        self.setting_vars = {
            "store_name": tk.StringVar(), "token": tk.StringVar(), "rub_per_cny": tk.StringVar(),
            "commission_percent": tk.StringVar(), "feishu_app_id": tk.StringVar(),
            "feishu_app_secret": tk.StringVar(), "feishu_chat_id": tk.StringVar(),
        }
        tk.Label(api, text="WB 店铺与 Token", bg="#FFFFFF", fg=TEXT,
                 font=("Microsoft YaHei UI", 14, "bold")).pack(anchor="w", padx=24, pady=(24, 14))
        self._setting_entry(api, "店铺名称", "store_name")
        self._setting_entry(api, "WB API Token", "token", show="•")
        self._setting_entry(api, "汇率：1 CNY = RUB", "rub_per_cny")
        self._setting_entry(api, "暂估平台综合费率 %", "commission_percent")
        actions = tk.Frame(api, bg="#FFFFFF")
        actions.pack(fill="x", padx=24, pady=18)
        self._button(actions, "保存设置", self._save_settings, True).pack(side="left")
        self._button(actions, "测试 WB API", self._test_api).pack(side="left", padx=10)
        tk.Label(
            api, text="Token 至少需要 Statistics、Promotion、Marketplace 相关读取权限。",
            bg="#FFFFFF", fg=MUTED, justify="left", font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=24)
        tk.Label(feishu, text="飞书周报", bg="#FFFFFF", fg=TEXT,
                 font=("Microsoft YaHei UI", 14, "bold")).pack(anchor="w", padx=24, pady=(24, 14))
        self._setting_entry(feishu, "App ID", "feishu_app_id")
        self._setting_entry(feishu, "App Secret", "feishu_app_secret", show="•")
        self._setting_entry(feishu, "群 Chat ID", "feishu_chat_id")
        actions2 = tk.Frame(feishu, bg="#FFFFFF")
        actions2.pack(fill="x", padx=24, pady=18)
        self._button(actions2, "测试飞书", self._test_feishu).pack(side="left")
        tk.Label(
            feishu, text="发送前请先把应用机器人加入目标群，并开通消息权限。",
            bg="#FFFFFF", fg=MUTED, justify="left", font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=24)

    def _setting_entry(self, parent: tk.Widget, label: str, key: str, show: str | None = None) -> None:
        holder = tk.Frame(parent, bg="#FFFFFF")
        holder.pack(fill="x", padx=24, pady=7)
        tk.Label(holder, text=label, bg="#FFFFFF", fg=MUTED,
                 font=("Microsoft YaHei UI", 8)).pack(anchor="w")
        ttk.Entry(holder, textvariable=self.setting_vars[key], show=show or "").pack(fill="x", pady=(4, 0), ipady=5)

    def _tree(self, parent: tk.Widget, columns: tuple[str, ...], headings: tuple[str, ...],
              widths: tuple[int, ...]) -> ttk.Treeview:
        holder = tk.Frame(parent, bg="#FFFFFF")
        holder.pack(fill="both", expand=True, padx=12, pady=12)
        tree = ttk.Treeview(holder, columns=columns, show="headings", style="WB.Treeview")
        xbar = ttk.Scrollbar(holder, orient="horizontal", command=tree.xview)
        ybar = ttk.Scrollbar(holder, orient="vertical", command=tree.yview)
        tree.configure(xscrollcommand=xbar.set, yscrollcommand=ybar.set)
        for column, heading, width in zip(columns, headings, widths):
            tree.heading(column, text=heading)
            tree.column(column, width=width, minwidth=50, anchor="e" if column not in {"day", "article", "warehouse", "status"} else "w")
        tree.grid(row=0, column=0, sticky="nsew")
        ybar.grid(row=0, column=1, sticky="ns")
        xbar.grid(row=1, column=0, sticky="ew")
        holder.rowconfigure(0, weight=1)
        holder.columnconfigure(0, weight=1)
        return tree

    def _load_settings(self) -> None:
        values = self.service.settings()
        for key, variable in self.setting_vars.items():
            variable.set(values.get(key, ""))

    def _save_settings(self) -> None:
        try:
            float(self.setting_vars["rub_per_cny"].get())
            float(self.setting_vars["commission_percent"].get())
            self.service.save_settings({key: value.get() for key, value in self.setting_vars.items()})
            self._status("WB 设置已加密保存到独立数据库。")
        except Exception as error:
            messagebox.showerror("保存失败", str(error), parent=self)

    def _sync_daily(self) -> None:
        self._run_async(
            lambda: self.service.sync(self.daily_from.get().strip(), self.daily_to.get().strip()),
            lambda result: (self._refresh_daily(), self._status(
                f"WB 同步完成：订单 {result['orders']}、商品广告 {result['ads']}、仓库 {result['warehouses']}。"
            )),
            "正在同步 WB 订单、商品广告和仓库…",
        )

    def _refresh_daily(self) -> None:
        try:
            rows = self.service.daily_rows(self.daily_from.get().strip(), self.daily_to.get().strip())
        except Exception as error:
            self._status(f"无法读取 WB 本地数据：{error}")
            return
        self._daily_rows = rows
        self.daily_tree.delete(*self.daily_tree.get_children())
        for index, row in enumerate(rows):
            revenue = float(row.get("revenue_rub") or 0)
            ad = float(row.get("spend_rub") or 0)
            tacos = ad / revenue * 100 if revenue else 0
            self.daily_tree.insert("", "end", iid=str(index), values=(
                row.get("day"), row.get("article") or "—", row.get("nm_id"), row.get("quantity"),
                _number(revenue), _number(ad), row.get("ad_orders"), f"{tacos:.2f}%",
                row.get("warehouse_name") or "—", _mode_label(row.get("warehouse_mode_resolved")),
                _money_or_dash(row.get("logistics_total_cny")), _money_or_dash(row.get("commission_cny")),
                _money_or_dash(row.get("profit_cny")), "暂估" if row.get("complete") else "缺成本/运费参数",
            ))
        complete = [row for row in rows if row.get("complete")]
        self.daily_cards["revenue"].set(f"{_number(sum(float(r.get('revenue_rub') or 0) for r in rows))} ₽")
        self.daily_cards["quantity"].set(f"{sum(int(r.get('quantity') or 0) for r in rows):,} 件")
        self.daily_cards["ads"].set(f"{_number(sum(float(r.get('spend_rub') or 0) for r in rows))} ₽")
        self.daily_cards["commission"].set(f"¥{_number(sum(float(r.get('commission_cny') or 0) for r in rows))}")
        self.daily_cards["profit"].set(f"¥{_number(sum(float(r.get('profit_cny') or 0) for r in complete))}" if complete else "—")
        self._status(f"已读取 {len(rows)} 条 WB 日期+商品记录；利润为实时经营暂估。")

    def _refresh_costs(self) -> None:
        rows = self.service.db.product_costs()
        self._cost_rows.clear()
        self.cost_tree.delete(*self.cost_tree.get_children())
        for row in rows:
            iid = str(row["nm_id"])
            self._cost_rows[iid] = row
            self.cost_tree.insert("", "end", iid=iid, values=(
                row.get("article") or "—", row.get("nm_id"), _money_or_dash(row.get("purchase_cost_cny")),
                _plain_or_dash(row.get("length_cm")), _plain_or_dash(row.get("width_cm")),
                _plain_or_dash(row.get("height_cm")), _plain_or_dash(row.get("weight_kg")), row.get("warehouse_mode"),
            ))

    def _save_cost(self) -> None:
        try:
            row = {
                "nm_id": int(self.cost_vars["nm_id"].get()), "article": self.cost_vars["article"].get(),
                "purchase_cost_cny": self.cost_vars["purchase"].get(), "length_cm": self.cost_vars["length"].get(),
                "width_cm": self.cost_vars["width"].get(), "height_cm": self.cost_vars["height"].get(),
                "weight_kg": self.cost_vars["weight"].get(), "warehouse_mode": self.cost_mode.get(),
            }
            self.service.db.save_product_cost(row)
            self._clear_cost()
            self._refresh_costs()
            self._status(f"WB 商品 {row['nm_id']} 的成本和运费参数已保存。")
        except Exception as error:
            messagebox.showerror("保存失败", f"请检查 nmId 和数字字段：{error}", parent=self)

    def _edit_cost(self, _event: tk.Event[Any]) -> None:
        selection = self.cost_tree.selection()
        if not selection:
            return
        row = self._cost_rows.get(selection[0])
        if not row:
            return
        mapping = {
            "nm_id": "nm_id", "article": "article", "purchase": "purchase_cost_cny",
            "length": "length_cm", "width": "width_cm", "height": "height_cm", "weight": "weight_kg",
        }
        for variable_key, row_key in mapping.items():
            self.cost_vars[variable_key].set("" if row.get(row_key) is None else str(row.get(row_key)))
        self.cost_mode.set(str(row.get("warehouse_mode") or "auto"))

    def _clear_cost(self) -> None:
        for variable in self.cost_vars.values():
            variable.set("")
        self.cost_mode.set("auto")

    def _refresh_weekly(self) -> None:
        for child in self.week_panel.winfo_children():
            child.destroy()
        try:
            summary = self.service.weekly_summary(self.week_from.get().strip(), self.week_to.get().strip())
        except Exception as error:
            tk.Label(self.week_panel, text=str(error), bg="#FFFFFF", fg="#B42318").pack(pady=40)
            return
        tk.Label(
            self.week_panel, text=f"{summary['store_name']} · {summary['date_from']} 至 {summary['date_to']}",
            bg="#FFFFFF", fg=TEXT, font=("Microsoft YaHei UI", 16, "bold"),
        ).pack(anchor="w", padx=28, pady=(28, 18))
        grid = tk.Frame(self.week_panel, bg="#FFFFFF")
        grid.pack(fill="x", padx=28)
        metrics = [
            ("销量", f"{summary['quantity']:,} 件"), ("销售额", f"¥{summary['revenue_cny']:,.2f}"),
            ("广告费", f"¥{summary['ad_spend_cny']:,.2f}"), ("暂估平台费", f"¥{summary['commission_cny']:,.2f}"),
            ("采购成本", f"¥{summary['purchase_cny']:,.2f}"), ("暂估物流", f"¥{summary['logistics_cny']:,.2f}"),
            ("暂估利润", f"¥{summary['profit_cny']:,.2f}"), ("缺参数商品", f"{summary['incomplete_products']} 个"),
        ]
        for index, (label, value) in enumerate(metrics):
            box = tk.Frame(grid, bg="#F9FAFB", highlightbackground=PALETTE.border, highlightthickness=1)
            box.grid(row=index // 4, column=index % 4, padx=5, pady=5, sticky="nsew")
            tk.Label(box, text=label, bg="#F9FAFB", fg=MUTED).pack(anchor="w", padx=16, pady=(13, 2))
            tk.Label(box, text=value, bg="#F9FAFB", fg=TEXT,
                     font=("Segoe UI", 15, "bold")).pack(anchor="w", padx=16, pady=(0, 13))
            grid.columnconfigure(index % 4, weight=1)
        tk.Label(
            self.week_panel,
            text="说明：周报使用实时订单、精确商品广告、可修改平台费率、采购成本与暂行物流估算；最终回款以 WB Finance 结算为准。",
            bg="#FFFFFF", fg="#B54708", font=("Microsoft YaHei UI", 8),
        ).pack(anchor="w", padx=30, pady=22)

    def _send_weekly(self) -> None:
        self._run_async(
            lambda: self.service.send_weekly_feishu(self.week_from.get().strip(), self.week_to.get().strip()),
            lambda result: self._status(result or "WB 周利润已发送到飞书群。"),
            "正在发送 WB 周利润卡片…",
        )

    def _test_api(self) -> None:
        self._save_settings()
        self._run_async(self.service.test_connection, lambda result: messagebox.showinfo("连接成功", result, parent=self),
                        "正在测试 WB API…")

    def _test_feishu(self) -> None:
        self._save_settings()
        self._run_async(self.service.test_feishu, lambda result: messagebox.showinfo("连接成功", result, parent=self),
                        "正在测试飞书连接…")

    def _run_async(self, work: Callable[[], Any], done: Callable[[Any], None], status: str) -> None:
        if self._busy:
            self._status("已有 WB 任务正在运行，请稍候。")
            return
        self._busy = True
        self.progress.start(10)
        self._status(status)

        def runner() -> None:
            try:
                result = work()
            except Exception as error:
                self.after(0, lambda: self._finish_error(error))
            else:
                self.after(0, lambda: self._finish_success(result, done))

        threading.Thread(target=runner, name="wb-background", daemon=True).start()

    def _finish_success(self, result: Any, done: Callable[[Any], None]) -> None:
        self._busy = False
        self.progress.stop()
        try:
            done(result)
        except Exception as error:
            self._finish_error(error)

    def _finish_error(self, error: Exception) -> None:
        self._busy = False
        self.progress.stop()
        self._status(f"WB 任务失败：{error}")
        messagebox.showerror("WB 操作失败", str(error), parent=self)

    def _status(self, text: str) -> None:
        self.status_var.set(text)


def _number(value: float) -> str:
    return f"{value:,.2f}".rstrip("0").rstrip(".")


def _money_or_dash(value: Any) -> str:
    return "—" if value is None else _number(float(value))


def _plain_or_dash(value: Any) -> str:
    return "—" if value is None else str(value)


def _mode_label(value: Any) -> str:
    return {"dongguan": "东莞仓", "overseas": "海外仓", "unknown": "待确认", "auto": "自动"}.get(str(value), str(value))
