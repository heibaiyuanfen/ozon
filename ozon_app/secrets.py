from __future__ import annotations

import base64
import ctypes
import sys
from ctypes import wintypes


class DATA_BLOB(ctypes.Structure):
    _fields_ = [("cbData", wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_byte))]


def protect(value: str) -> str:
    if not value:
        return ""
    if sys.platform != "win32":
        return "plain:" + value
    raw_value = value.encode("utf-8")
    source_buffer = ctypes.create_string_buffer(raw_value)
    source = DATA_BLOB(len(raw_value), ctypes.cast(source_buffer, ctypes.POINTER(ctypes.c_byte)))
    target = DATA_BLOB()
    if not ctypes.windll.crypt32.CryptProtectData(
        ctypes.byref(source), "OzonAnalytics", None, None, None, 0, ctypes.byref(target)
    ):
        raise ctypes.WinError()
    try:
        encrypted = ctypes.string_at(target.pbData, target.cbData)
        return "dpapi:" + base64.b64encode(encrypted).decode("ascii")
    finally:
        ctypes.windll.kernel32.LocalFree(target.pbData)


def unprotect(value: str) -> str:
    if not value:
        return ""
    if value.startswith("plain:"):
        return value[6:]
    if not value.startswith("dpapi:") or sys.platform != "win32":
        return value
    encrypted = base64.b64decode(value[6:])
    source_buffer = ctypes.create_string_buffer(encrypted)
    source = DATA_BLOB(len(encrypted), ctypes.cast(source_buffer, ctypes.POINTER(ctypes.c_byte)))
    target = DATA_BLOB()
    if not ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(source), None, None, None, None, 0, ctypes.byref(target)
    ):
        raise ctypes.WinError()
    try:
        return ctypes.string_at(target.pbData, target.cbData).decode("utf-8")
    finally:
        ctypes.windll.kernel32.LocalFree(target.pbData)
