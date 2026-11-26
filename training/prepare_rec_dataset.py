#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 PPOCRLabel 导出的识别标注整理成 PaddleOCR 训练格式，并检查字典覆盖。

PPOCRLabel 导出目录里通常有 rec_gt_train.txt / rec_gt_val.txt，每行：
    图片路径<TAB>文本
以及 crop_img/ 下的行图片。

用法：
    python prepare_rec_dataset.py --input <导出目录> --out train_data
    python prepare_rec_dataset.py --input <导出目录> --out train_data --dict ppocr_keys.txt

输出：
    train_data/rec_gt_train.txt
    train_data/rec_gt_val.txt
每行同样是「图片绝对路径<TAB>文本」，可直接给 PaddleOCR 的 label_file_list。
"""

import argparse
import os
import random
import sys

CANDIDATE_FILES = ["rec_gt_train.txt", "rec_gt_val.txt", "rec_gt.txt"]


def load_dict(path):
    """读取 PaddleOCR 字典（每行一个字符），返回字符集合；空文件返回 None。"""
    if not path:
        return None
    chars = set()
    with open(path, "r", encoding="utf-8-sig") as f:
        for line in f:
            line = line.rstrip("\r\n")
            if not line:
                continue
            if line == " ":
                # use_space_char 时字典末尾的空格行，不算字符
                continue
            chars.add(line[0])
    return chars or None


def resolve_image(raw, base):
    """把标注里的图片路径解析成存在的绝对路径。"""
    raw = raw.strip()
    if not raw:
        return None
    candidates = []
    if os.path.isabs(raw):
        candidates.append(raw)
    else:
        candidates.append(os.path.join(base, raw))
    # PPOCRLabel 有时只写文件名，图片实际在 crop_img/ 下
    candidates.append(os.path.join(base, "crop_img", os.path.basename(raw)))
    for path in candidates:
        if os.path.exists(path):
            return os.path.abspath(path)
    return None


def read_labels(path, base):
    rows = []
    with open(path, "r", encoding="utf-8-sig", newline="") as f:
        for lineno, line in enumerate(f, 1):
            line = line.rstrip("\r\n")
            if not line.strip():
                continue
            parts = line.split("\t")
            if len(parts) < 2:
                print("{}:{} 跳过（没有 TAB 分隔）".format(path, lineno), file=sys.stderr)
                continue
            image = resolve_image(parts[0], base)
            label = " ".join(p.strip() for p in parts[1:]).strip()
            if not label:
                continue
            if image is None:
                print("缺少图片：{}（{}:{}）".format(parts[0], path, lineno), file=sys.stderr)
                continue
            rows.append((image, label))
    return rows


def write_labels(path, rows):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for image, label in rows:
            f.write("{}\t{}\n".format(image, label))


def main():
    parser = argparse.ArgumentParser(description="整理 PaddleOCR 识别训练数据")
    parser.add_argument("--input", required=True, help="PPOCRLabel 导出目录")
    parser.add_argument("--out", default="train_data", help="输出目录")
    parser.add_argument("--dict", default=None, help="字典文件（可选，用于检查字符覆盖）")
    parser.add_argument("--val-ratio", type=float, default=0.1, help="验证集比例")
    parser.add_argument("--seed", type=int, default=2024, help="切分随机种子")
    parser.add_argument("--max-len", type=int, default=50, help="标签最大长度，超过只警告")
    parser.add_argument("--drop-unknown", action="store_true", help="丢掉含字典外字符的样本")
    args = parser.parse_args()

    if not os.path.isdir(args.input):
        sys.exit("输入目录不存在：{}".format(args.input))

    dict_chars = load_dict(args.dict)

    rows = []
    seen_files = []
    for name in CANDIDATE_FILES:
        path = os.path.join(args.input, name)
        if os.path.exists(path):
            seen_files.append(path)
            rows.extend(read_labels(path, args.input))
    if not seen_files:
        sys.exit(
            "没有找到 rec_gt_train.txt / rec_gt_val.txt / rec_gt.txt，"
            "请确认这是 PPOCRLabel 导出的识别数据集目录"
        )

    # 同一张图 + 同一标签只保留一次
    seen = set()
    unique = []
    for image, label in rows:
        key = (image, label)
        if key in seen:
            continue
        seen.add(key)
        unique.append((image, label))
    rows = unique
    if not rows:
        sys.exit("没有读到任何有效标注行")

    unknown_total = {}
    kept = []
    for image, label in rows:
        if len(label) > args.max_len:
            print("标签过长（{}）：{} {}".format(len(label), image, label), file=sys.stderr)
        bad = []
        if dict_chars is not None:
            for ch in label:
                if ch == " ":
                    continue
                if ch not in dict_chars:
                    bad.append(ch)
        if bad:
            for ch in set(bad):
                unknown_total[ch] = unknown_total.get(ch, 0) + 1
            if args.drop_unknown:
                continue
        kept.append((image, label))

    if unknown_total:
        summary = "、".join(
            "{}×{}".format(ch, n)
            for ch, n in sorted(unknown_total.items(), key=lambda kv: -kv[1])
        )
        print("字典外字符（出现在 {} 行）：{}".format(len(kept) if args.drop_unknown else len(rows), summary))
        print("提示：把这些字符加进关键词设置后重新导出字典，或给字典文件手工补行。")
        if not args.drop_unknown:
            print("当前未丢弃这些样本；如要丢弃加 --drop-unknown。")

    if not kept:
        sys.exit("没有可用样本（可能全部被 --drop-unknown 丢弃）")

    random.Random(args.seed).shuffle(kept)
    val_count = 0
    if args.val_ratio > 0 and len(kept) >= 10:
        val_count = max(1, int(len(kept) * args.val_ratio))
    train_rows = kept[val_count:]
    val_rows = kept[:val_count]

    os.makedirs(args.out, exist_ok=True)
    write_labels(os.path.join(args.out, "rec_gt_train.txt"), train_rows)
    write_labels(os.path.join(args.out, "rec_gt_val.txt"), val_rows)
    print(
        "完成：训练 {} 行，验证 {} 行 → {}".format(
            len(train_rows), len(val_rows), os.path.abspath(args.out)
        )
    )
    if val_count == 0:
        print("样本少于 10 条，未生成验证集；训练时请把 Eval 指向训练集或调小 val-ratio。")


if __name__ == "__main__":
    main()
