from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.exports import export_ai_docx, export_ai_pdf


class ExportTests(unittest.TestCase):
    def test_ai_report_exports_word_and_pdf(self) -> None:
        try:
            from docx import Document
            from pypdf import PdfReader
        except ImportError:
            self.skipTest("document export dependencies are not installed")
        sample = "# 经营结论\n销售保持增长。\n\n## 优先行动\n1. **控制 ACOS**\n- 复查高花费活动"
        with TemporaryDirectory() as folder:
            docx_path = Path(folder) / "report.docx"
            pdf_path = Path(folder) / "report.pdf"
            export_ai_docx(docx_path, sample, "2026-08-01", "2026-08-13", "test-model")
            export_ai_pdf(pdf_path, sample, "2026-08-01", "2026-08-13", "test-model")
            document = Document(docx_path)
            text = "\n".join(paragraph.text for paragraph in document.paragraphs)
            self.assertIn("Ozon AI 经营分析报告", text)
            self.assertIn("经营结论", text)
            self.assertTrue(pdf_path.read_bytes().startswith(b"%PDF"))
            self.assertGreaterEqual(len(PdfReader(pdf_path).pages), 1)


if __name__ == "__main__":
    unittest.main()
