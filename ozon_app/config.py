from dataclasses import dataclass


APP_NAME = "Ozon 广告与销量分析"
APP_VERSION = "0.15.3"
SELLER_BASE_URL = "https://api-seller.ozon.ru"
PERFORMANCE_BASE_URL = "https://api-performance.ozon.ru"


@dataclass(frozen=True)
class Palette:
    """Central color tokens for the local desktop workbench."""

    bg: str = "#F6F8FC"
    panel: str = "#FFFFFF"
    panel_alt: str = "#F8FAFD"
    text: str = "#172B4D"
    muted: str = "#667085"
    border: str = "#E5EAF2"
    primary: str = "#2F6FED"
    primary_dark: str = "#175CD3"
    light_blue: str = "#EAF2FF"
    green: str = "#12B76A"
    amber: str = "#F79009"
    red: str = "#D92D20"
    sidebar: str = "#FFFFFF"
    sidebar_hover: str = "#F3F6FB"
    sidebar_active: str = "#EAF2FF"
    sidebar_text: str = "#344054"
    sidebar_muted: str = "#98A2B3"


PALETTE = Palette()
