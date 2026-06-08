# tidy-verify Step 4：从 source 路径里抽显式时间字符串，与 target 桶比对。
# 暴露「文件名/目录隐含时间 ≠ 归档桶」的孤儿（通常 EXIF 缺失 + mtime 不准）。
#
# 用法（SOURCE_ROOT MUST 通过环境变量传，不可用 `-v`：awk -v 会把 `\U` `\P`
# 当 escape 序列吃掉 Windows 路径里的反斜杠）：
#   SOURCE_ROOT='D:\Users\Public\Pictures\2006\' awk -f filename_conflict.awk copy_lines.log
#
# 剥掉前缀避免源根目录里的年份字串干扰 parse_path。
#
# 抽取模式（按优先级）：
#   1. YYYY[-_./ ]?MM       YYYY ∈ 1995..2030, MM 01..12
#   2. YYYYMMDD             8 连续数字
#   3. YY-MM-DD             YY<50 → 20YY, ≥50 → 19YY（含 DD 减少假阳）
#
# 输出：
#   每条 DIFFER 一行：`DIFFER \t name=YYYY:MM \t tgt=YYYY:MM \t <source>`
#   末尾 with_name_time=<路径含可识别时间的总数>

function parse_path(s,    y, mm) {
    if (match(s, /(199[5-9]|20[0-2][0-9]|2030)[-_.\/ ]?(0[1-9]|1[012])/, mm)) {
        return mm[1] ":" mm[2]
    }
    if (match(s, /(199[5-9]|20[0-2][0-9]|2030)(0[1-9]|1[012])(0[1-9]|[12][0-9]|3[01])/, mm)) {
        return mm[1] ":" mm[2]
    }
    if (match(s, /(^|[^0-9])([0-9][0-9])-(0[1-9]|1[012])-(0[1-9]|[12][0-9]|3[01])([^0-9]|$)/, mm)) {
        y = mm[2] + 0
        if (y < 50) y += 2000
        else y += 1900
        return sprintf("%04d:%s", y, mm[3])
    }
    return ""
}

BEGIN {
    SOURCE_ROOT = ENVIRON["SOURCE_ROOT"]
    if (SOURCE_ROOT == "") {
        print "ERROR: set SOURCE_ROOT env var (with trailing path separator)" > "/dev/stderr"
        exit 2
    }
}

{
    si = index($0, "source=")
    ti = index($0, " target=")
    if (si == 0 || ti == 0) next
    s = substr($0, si + 7, ti - si - 7)
    t = substr($0, ti + 8)

    # target 桶 \YYYY\MM\
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

    rel = s
    if (index(rel, SOURCE_ROOT) == 1) {
        rel = substr(rel, length(SOURCE_ROOT) + 1)
    }
    nt = parse_path(rel)
    if (nt == "") next
    if (nt != tgt) {
        print "DIFFER\tname=" nt "\ttgt=" tgt "\t" s
    }
    counted++
}

END {
    print "---"
    print "with_name_time=" counted
}
