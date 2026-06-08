# tidy-verify Step 3：EXIF 年月 vs tidymedia 推断的 target 桶 YYYY/MM。
#
# 输入（顺序固定，pass 1 / pass 2）：
#   1. exif.tsv          每行: path \t DateTimeOriginal \t CreateDate \t FileModifyDate
#                        exiftool -p '$Directory/$FileName\t$DateTimeOriginal\t$CreateDate\t$FileModifyDate'
#                        空字段以 `-` 输出
#   2. copy_lines.log    tidymedia --log-level=debug move --dry-run 抽出的
#                        `operation="copy_file"` 行，每行含 source=... target=...
#
# 输出：
#   每条 MISMATCH 一行：`MISMATCH \t exp=YYYY:MM \t tgt=YYYY:MM \t from=DTO|CreateDate|FsMtime|NONE|MISSING \t <source>`
#   末尾 compared=<对比的 copy_file 行数>
#
# 注意：
#   - exiftool 路径用 `/`、tidymedia 用 `\`，第一 pass 已规范到 `\` 作 key
#   - 变量名避开 gawk 内建：MUST NOT 用 `exp` `log` `length` 等

BEGIN { FS = "\t" }

# Pass 1: exif.tsv — 建 expected map
FNR == NR {
    p = $1
    gsub("/", "\\", p)
    pick = ""
    src = ""
    for (i = 2; i <= 4; i++) {
        v = $i
        # exiftool 时间格式 YYYY:MM:DD HH:MM:SS，前 7 字符即 YYYY:MM
        if (length(v) >= 7 && substr(v, 5, 1) == ":") {
            pick = substr(v, 1, 7)
            if (i == 2) src = "DTO"
            else if (i == 3) src = "CreateDate"
            else src = "FsMtime"
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
    if (ex != tgt) {
        print "MISMATCH\texp=" ex "\ttgt=" tgt "\tfrom=" src_label "\t" s
    }
    total++
}

END {
    print "---"
    print "compared=" total
}
