#!/usr/bin/env python3
"""从固定版本的 ChinaGeoJson 本地目录生成城市索引，不重复保存边界坐标。"""

import argparse
import json
import logging
from pathlib import Path


def build_index(upstream: Path) -> None:
    """逐点验证省级外环一致后，原子写出城市在外环数组中的位置。"""
    target = Path(__file__).resolve().parents[1] / "src/assets/regions"
    regions = json.loads((target / "china.json").read_text(encoding="utf-8"))
    index = {}
    for region in regions:
        if region["id"] == "中华人民共和国":
            continue
        source = upstream / "province" / f"{region['id']}.json"
        data = json.loads(source.read_text(encoding="utf-8"))
        polygons = []
        cities = []
        for feature in data["features"]:
            geometry = feature.get("geometry")
            if not geometry:
                continue
            if geometry["type"] not in ("Polygon", "MultiPolygon"):
                raise ValueError(f"不支持的边界类型：{source}，{geometry['type']}")
            faces = [geometry["coordinates"]] if geometry["type"] == "Polygon" else geometry["coordinates"]
            rings = [face[0] for face in faces if face]
            properties = feature["properties"]
            # 直辖市、港澳的下级为区县，台湾源文件仅包含省级边界，均不冒充市级数据。
            if properties.get("level") == "city" and rings:
                cities.append({
                    "id": str(properties["adcode"]),
                    "name": properties["name"],
                    "polygonStart": len(polygons),
                    "polygonCount": len(rings),
                })
            polygons.extend(rings)
        if polygons != region["polygons"]:
            raise ValueError(f"省级边界与当前离线数据不一致，禁止生成索引：{region['id']}")
        if len({city["id"] for city in cities}) != len(cities):
            raise ValueError(f"城市行政代码重复：{region['id']}")
        index[region["id"]] = cities
        logging.info("已核对 %s：%d 个市级区域", region["id"], len(cities))
    output = target / "cities.json"
    temporary = output.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(index, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf-8")
    temporary.replace(output)
    logging.info("已生成 %s，共 %d 个市级区域", output, sum(map(len, index.values())))


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path, help="ChinaGeoJson 固定版本的本地目录（包含 province 子目录）")
    try:
        build_index(parser.parse_args().upstream)
    except Exception:
        logging.exception("城市索引生成失败，原索引保持不变")
        raise SystemExit(1)
