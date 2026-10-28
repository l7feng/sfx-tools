"""安装试用前的一致性备份；只读源库，不覆盖既有目标。"""
import argparse
import hashlib
import json
import os
import sqlite3
from pathlib import Path


def backup(source: Path, output: Path):
    source = source.resolve(strict=True)
    output = output.absolute()
    if source == output or source.parent == output.parent:
        raise ValueError("备份必须放在应用数据目录之外")
    output.parent.mkdir(parents=True, exist_ok=True)
    # 独占创建目标，防止误覆盖以前的备份或正式库。
    with output.open("xb"):
        pass
    with sqlite3.connect(source.as_uri() + "?mode=ro", uri=True, timeout=10) as original:
        with sqlite3.connect(output) as target:
            original.backup(target, pages=128, sleep=0.05)
            integrity = target.execute("PRAGMA integrity_check").fetchone()[0]
            if integrity != "ok" or target.execute("PRAGMA foreign_key_check").fetchall():
                raise ValueError("备份完整性校验未通过，请保留失败文件调查")
            counts = {}
            for name in ("assets", "jobs", "user_metadata", "asset_fingerprints", "import_history"):
                exists = target.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,)).fetchone()
                if exists:
                    counts[name] = target.execute(f'SELECT COUNT(*) FROM "{name}"').fetchone()[0]
            report = {"source": str(source), "backup": str(output), "integrity": integrity,
                      "schema": target.execute("PRAGMA user_version").fetchone()[0], "table_counts": counts}
    # 上下文管理器提交事务，但明确关闭所有连接，避免恢复前仍持有文件句柄。
    original.close()
    target.close()
    report["sha256"] = hashlib.sha256(output.read_bytes()).hexdigest()
    with output.with_suffix(".verification.json").open("x", encoding="utf-8") as file:
        json.dump(report, file, ensure_ascii=False, indent=2)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(os.environ["LOCALAPPDATA"]) / "local.soundworkbench.desktop/catalog.sqlite")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(backup(args.source, args.output), ensure_ascii=False))
