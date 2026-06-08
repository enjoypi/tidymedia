# tidy-verify Step 3：EXIF 年月 vs tidymedia 推断的 target 桶 YYYY/MM。
#
# 输入（顺序固定，pass 1 / pass 2）：
#   1. exif.tsv          每行 8 列（顺序对齐 tidymedia P0..P4）:
#                        path \t DTO \t QT:CreationDate \t QT:CreateDate \t CreateDate \t FileModifyDate \t Make \t Model
#                        空字段以 `-` 输出（exiftool -T 默认）
#   2. copy_lines.log    tidymedia --log-level=debug move --dry-run 抽出的
#                        `operation="copy_file"` 行，每行含 source=... target=...
#
# 输出：
#   每条 MISMATCH 一行：`MISMATCH \t exp=YYYY:MM \t tgt=YYYY:MM \t from=DTO|QTCreationDate|QTCreateDate|CreateDate|FsMtime|NONE|MISSING \t make=<Make> \t model=<Model> \t <source>`
#   末尾 compared=<对比的 copy_file 行数>
#   from 优先级顺序：DTO > QTCreationDate > QTCreateDate > CreateDate > FsMtime，
#   与 tidymedia P0..P4 对齐；exiftool 拿到容器时间而 tidymedia 走 FsMtime 兜底
#   即可检出（如 pnot 老 QuickTime nom-exif 漏读）。
#   Make/Model 是 Step 5 证据卡片 pattern 分类必备字段（识别相机时钟未设、机型发布日冲突）
#
# 注意：
#   - exiftool 路径用 `/`、tidymedia 用 `\`，第一 pass 已规范到 `\` 作 key
#   - 变量名避开 gawk 内建：MUST NOT 用 `exp` `log` `length` 等

BEGIN { FS = "\t" }

# Pass 1: exif.tsv — 建 expected map
# 字段顺序固定（02_extract_exif.sh 同步维护）：
#   2 DTO  3 QT:CreationDate  4 QT:CreateDate  5 CreateDate  6 FileModifyDate  7 Make  8 Model
# 扫 2..6 取第一条合法 YYYY:MM 作 expected；标签对齐 from_label[]
FNR == NR {
    p = $1
    gsub("/", "\\", p)
    pick = ""
    src = ""
    # 索引对齐 $2..$6
    from_label[2] = "DTO"
    from_label[3] = "QTCreationDate"
    from_label[4] = "QTCreateDate"
    from_label[5] = "CreateDate"
    from_label[6] = "FsMtime"
    for (i = 2; i <= 6; i++) {
        v = $i
        # exiftool 时间格式 YYYY:MM:DD HH:MM:SS，前 7 字符即 YYYY:MM
        if (length(v) >= 7 && substr(v, 5, 1) == ":") {
            pick = substr(v, 1, 7)
            src = from_label[i]
            break
        }
    }
    if (pick == "") {
        expected[p] = "NONE"
        from[p] = "NONE"
    } else {
        expected[p] = pick
        from[p] = src
    }
    make[p] = ($7 == "" || $7 == "-") ? "-" : $7
    model[p] = ($8 == "" || $8 == "-") ? "-" : $8
    next
}

# Pass 2: copy_lines.log — 对每行抽 source/target 比对
{
    src_idx = index($0, "source=")
    tgt_idx = index($0, " target=")
    if (src_idx == 0 || tgt_idx == 0) next
    s = substr($0, src_idx + 7, tgt_idx - src_idx - 7)
    t = substr($0, tgt_idx + 8)

    # 从 target 路径里抽第一个 \YYYY\MM\ 段作为桶
    n = split(t, parts, "\\")
    tgt = "NO_BUCKET"
    for (k = 1; k <= n - 2; k++) {
        if (length(parts[k]) == 4 && length(parts[k+1]) == 2 \
            && parts[k] ~ /^[0-9][0-9][0-9][0-9]$/ \
            && parts[k+1] ~ /^[0-9][0-9]$/) {
            tgt = parts[k] ":" parts[k+1]
            break
        }
    }

    ex = "MISSING"
    if (s in expected) ex = expected[s]
    src_label = "MISSING"
    if (s in from) src_label = from[s]
    mk = "-"
    md = "-"
    if (s in make) mk = make[s]
    if (s in model) md = model[s]
    if (ex != tgt) {
        print "MISMATCH\texp=" ex "\ttgt=" tgt "\tfrom=" src_label "\tmake=" mk "\tmodel=" md "\t" s
    }
    total++
}

END {
    print "---"
    print "compared=" total
}
