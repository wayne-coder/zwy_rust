# 训练专属 OCR 识别模型（PaddleOCR）

目标是微调 PP-OCRv4 的中文识别模型（rec），让它专门擅长识别注单文本：数字、金额、
玩法词、生肖、符号。训练完成后导出 ONNX，由程序离线调用；模型缺失时仍回退 Windows OCR。

> 数据量参考官方微调文档：**不换字典约 5000 行样本**；换自定义小字典需要更多，
> 官方明确「不建议」换字典。所以建议先用**官方中文字典**微调，字符限定交给程序里的
> 词库纠错；等数据量足够（5000+ 行）再考虑小字典专模。

## 目录约定

```
training/
  README.md               本说明
  prepare_rec_dataset.py  整理标注 → 训练格式 + 检查字典覆盖
  train_data/             由脚本生成（不提交）
  output/                 训练输出（不提交）
  pretrain_models/        预训练模型（不提交）
  inference/              导出目录（不提交）
```

## 1. 收集行样本

识别模型是**单行**模型：一张图只能有一行文字。

- 数量：起步 1000 行，目标 5000 行以上。
- 与最终使用环境一致：同一部手机 / 微信字号 / 深浅色主题 / 缩放比例。
- 每行单独裁剪，左右留 2–4px，不要切掉字的边缘；建议图片高 ≥ 32px。
- 覆盖所有玩法词、12 生肖、金额写法（10 / 100 / 1,000）、空格与换行形式、赔率、
  颜色字、半透明字。
- 混入难样本：低分辨率、压缩、缩小、轻微模糊、彩色背景。
- 不要包含头像、昵称、时间、聊天气泡边缘——除非实际识别时它们也在图里。

没有真实样本时，可以先用系统字体渲染「词库句子」生成合成行图（随机字体、字号、
背景、噪声、模糊），但它只能兜底，真实样本不可替代。

## 2. 标注：PPOCRLabel

PaddleOCR 自带标注工具，支持自动检测预标注，人工改正，导出识别数据集。

```bash
git clone -b v2.8.1 https://github.com/PaddlePaddle/PaddleOCR.git
cd PaddleOCR
pip install -r requirements.txt
python PPOCRLabel/PPOCRLabel.py
```

在 PPOCRLabel 里打开截图 → 自动标注 → 逐行改字 → 导出。导出目录里的
`rec_gt_train.txt` 就是「图片路径 TAB 文本」的识别标注。

## 3. 字典

- **方案 A（推荐先做）**：不换字典，直接用官方 `ppocr/utils/ppocr_keys_v1.txt`。
  预训练分类头不用重建，最稳；输出字符不限，但后处理纠错会按词库收敛。
- **方案 B（字符限定模型）**：用本程序导出的小字典，只能输出词库内的字符。

导出小字典（字符集 = 当前关键词设置 + 生肖 + 数字符号）：

```bash
账无忧.exe --export-ocr-dict ppocr_keys.txt
```

训练与推理必须使用**同一份字典**；以后改了关键词词表，要重新导出并重新训练，
或者沿用官方字典。方案 B 会改变分类头维度，预训练模型的输出层会被重新初始化
（日志里出现 shape 不匹配是正常的），因此需要更多样本。

## 4. 准备数据集

```bash
python training/prepare_rec_dataset.py \
  --input <PPOCRLabel 导出目录> \
  --out training/train_data \
  --dict ppocr_keys.txt        # 方案 A 不传 --dict
```

脚本会：

- 汇总 `rec_gt_train.txt` / `rec_gt_val.txt`，校验图片是否存在；
- 报告标签里落在字典外的字符（有的话训练会报错）；
- 9:1 切分训练 / 验证（验证集来自不同截图更可靠，避免同图裁剪泄漏）；
- 输出 `train_data/rec_gt_train.txt`、`train_data/rec_gt_val.txt`。

## 5. 环境与微调

- Python 3.8–3.12；有 NVIDIA 显卡装 `paddlepaddle-gpu`，否则 `paddlepaddle`（2.6.x）。
- 下载训练模型：
  `https://paddleocr.bj.bcebos.com/PP-OCRv4/chinese/ch_PP-OCRv4_rec_train.tar`
  解压到 `training/pretrain_models/`。

在 PaddleOCR 仓库根目录执行（路径按实际调整）：

```bash
python tools/train.py -c configs/rec/PP-OCRv4/ch_PP-OCRv4_rec_hgnet.yml \
  -o Global.pretrained_model=/abs/path/training/pretrain_models/ch_PP-OCRv4_rec_train/best_accuracy \
     Global.save_model_dir=/abs/path/training/output/rec_finetune \
     Global.max_text_length=50 \
     Train.dataset.label_file_list=['/abs/path/training/train_data/rec_gt_train.txt'] \
     Eval.dataset.label_file_list=['/abs/path/training/train_data/rec_gt_val.txt'] \
     Train.loader.batch_size_per_card=32 \
     Eval.loader.batch_size_per_card=32
```

方案 B 再追加：

```bash
     Global.character_dict_path=/abs/path/training/ppocr_keys.txt
```

要点：

- 单卡、batch 32 时学习率建议 1e-4 ~ 5e-4；数据少就调小（官方从 1e-3 起）。
- 每轮看验证集 `acc`，取 `output/rec_finetune/best_accuracy`。
- 训练机与整理数据的机器保持同一系统（label 里写的是绝对路径；换到 Linux 需改成相对路径）。
- 若加载预训练模型报结构不匹配，换 tar 包里另一个 `.pdparams`（如 `student`）再试。

## 6. 导出 ONNX

```bash
python tools/export_model.py -c configs/rec/PP-OCRv4/ch_PP-OCRv4_rec_hgnet.yml \
  -o Global.pretrained_model=/abs/path/training/output/rec_finetune/best_accuracy \
     Global.save_inference_dir=/abs/path/training/inference/rec

paddle2onnx --model_dir /abs/path/training/inference/rec \
  --model_filename inference.pdmodel --params_filename inference.pdiparams \
  --save_file training/rec.onnx --opset_version 11 --enable_onnx_checker True
```

检测模型直接用官方 `ch_PP-OCRv4_det_infer` 转换的 ONNX（不需要微调）：

```
https://paddleocr.bj.bcebos.com/PP-OCRv4/chinese/ch_PP-OCRv4_det_infer.tar
```

## 7. 程序接入（模型就绪后实现）

模型目录约定：

```
%LOCALAPPDATA%\WechatViewer\paddleocr\
  det.onnx
  rec.onnx
  ppocr_keys.txt      （方案 B 的字典；方案 A 用官方字典也放这里）
```

程序检测到目录完整时使用 PaddleOCR，否则回退 Windows OCR。推理库二选一：

- `tract`（纯 Rust）：保持单文件 exe，编译慢，需确认模型算子支持；
- `ort` + onnxruntime.dll：跑得快、兼容性好，但要多带一个 DLL（或首次运行释放）。

## 8. 常见问题

- **标签含字典外字符**：训练直接报错；先用 `prepare_rec_dataset.py` 检查。
- **验证 acc 很高但实测差**：训练/验证来自同一张截图的行会泄漏，按截图切分。
- **过拟合**：真实样本太少时，混入合成数据和通用中文数据，或减小学习率、早停。
- **空间识别**：标签里有空格时，配置需要 `Global.use_space_char=true`（默认已开）。
