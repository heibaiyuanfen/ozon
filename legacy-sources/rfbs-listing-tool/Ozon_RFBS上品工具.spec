# -*- mode: python ; coding: utf-8 -*-

from pathlib import Path
import sys

from PyInstaller.utils.hooks import collect_all


tool_dir = Path(SPEC).resolve().parent

datas = []
binaries = []
hiddenimports = []
for package_name in ("playwright", "alibabacloud_oss_v2", "openpyxl"):
    package_datas, package_binaries, package_hiddenimports = collect_all(package_name)
    datas += package_datas
    binaries += package_binaries
    hiddenimports += package_hiddenimports

# Python 3.13 + PyInstaller may discover Tcl/Tk subdirectories while omitting
# the root scripts (notably init.tcl and tk.tcl). Include both runtime trees
# explicitly so the packaged Tkinter GUI can initialize on a clean machine.
tcl_root = Path(sys.base_prefix) / "tcl"
datas += [
    (str(tcl_root / "tcl8.6"), "_tcl_data"),
    (str(tcl_root / "tk8.6"), "_tk_data"),
]

a = Analysis(
    [str(tool_dir / "main.py")],
    pathex=[str(tool_dir)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["pandas", "numpy", "aiohttp"],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="Ozon_RFBS上品工具",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="Ozon_RFBS上品工具",
)
