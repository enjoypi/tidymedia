use super::*;

// ── find_xmp_packet ──

#[test]
fn find_xmp_packet_in_jpeg_head() {
    let head = b"\xff\xd8\xff\xe1...<x:xmpmeta xmlns:x='adobe:ns:meta/'>body</x:xmpmeta>tail";
    let packet = find_xmp_packet(head).expect("packet found");
    assert!(packet.starts_with("<x:xmpmeta"));
    assert!(packet.ends_with("</x:xmpmeta>"));
    assert!(packet.contains("body"));
}

/// 起始 marker 缺失（既无 `<x:xmpmeta` 也无 `</x:xmpmeta>`）→ None。
#[test]
fn find_xmp_packet_missing_start_returns_none() {
    assert!(find_xmp_packet(b"random bytes without marker").is_none());
}

/// 有起始 marker 但无结束 marker（packet 被截断）→ None。
/// 注意：内部 close 字符串 "</x:xmpmeta>" 故意嵌在最后避免命中。
#[test]
fn find_xmp_packet_missing_end_returns_none() {
    let buf = b"prefix<x:xmpmeta body without close";
    assert!(find_xmp_packet(buf).is_none());
}

/// packet 字节流非 UTF-8（即便 marker 完整）→ `from_utf8` 验证失败 → None。
#[test]
fn find_xmp_packet_non_utf8_returns_none() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"<x:xmpmeta ");
    buf.push(0xff); // 非法 UTF-8 起始字节
    buf.push(0xfe);
    buf.extend_from_slice(b"</x:xmpmeta>");
    assert!(find_xmp_packet(&buf).is_none());
}

#[test]
fn find_xmp_packet_empty_buf_returns_none() {
    assert!(find_xmp_packet(b"").is_none());
}

// ── parse_xmp_dates ──

#[test]
fn parse_xmp_dates_attribute_double_quoted() {
    let xml = r#"<rdf:Description photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>"#;
    let dates = parse_xmp_dates(xml);
    let dt = dates.photoshop_date_created.expect("photoshop key present");
    // 14:30 +08:00 = 06:30 UTC = 1714545000
    assert_eq!(dt.timestamp(), 1_714_545_000);
    assert!(dates.xmp_create_date.is_none());
}

/// exiftool Shorthand 模式输出的单引号 attribute 也要识别（与 fixture 同形态）。
#[test]
fn parse_xmp_dates_attribute_single_quoted() {
    let xml = "<rdf:Description xmp:CreateDate='2008-10-31T09:15:01+08:00'/>";
    let dates = parse_xmp_dates(xml);
    let dt = dates.xmp_create_date.expect("xmp:CreateDate present");
    // 09:15:01 +08:00 = 01:15:01 UTC = 1225415701
    assert_eq!(dt.timestamp(), 1_225_415_701);
    assert!(dates.photoshop_date_created.is_none());
}

/// 同时含两个键 → 两字段都有值。
#[test]
fn parse_xmp_dates_both_keys_present() {
    let xml = r#"<rdf:Description
        photoshop:DateCreated="2024-05-01T14:30:00+08:00"
        xmp:CreateDate="2024-05-02T15:30:00+00:00"/>"#;
    let dates = parse_xmp_dates(xml);
    assert_eq!(
        dates.photoshop_date_created.unwrap().timestamp(),
        1_714_545_000
    );
    assert_eq!(dates.xmp_create_date.unwrap().timestamp(), 1_714_663_800);
}

/// 同形态字面量藏在 XML 注释里 → 正文优先；注释里的不能误命中。
#[test]
fn parse_xmp_dates_skips_xml_comment() {
    let xml = r#"<!-- photoshop:DateCreated="2020-01-01T00:00:00Z" -->
<rdf:Description photoshop:DateCreated="2024-05-01T14:30:00+08:00"/>"#;
    let dates = parse_xmp_dates(xml);
    assert_eq!(
        dates.photoshop_date_created.unwrap().timestamp(),
        1_714_545_000
    );
}

#[test]
fn parse_xmp_dates_no_keys_returns_default() {
    let dates = parse_xmp_dates("<rdf:Description rdf:about=''/>");
    assert_eq!(dates, XmpDates::default());
}

/// `=` 后既不是 `"` 也不是 `'` → 不当作 attribute → None。
#[test]
fn parse_xmp_dates_non_quote_after_equals_returns_none() {
    // 等号后是 `<` —— 不合法 XML，但解析器不应 panic 也不应解析出值。
    let xml = r"<x photoshop:DateCreated=<bad/>";
    assert!(parse_xmp_dates(xml).photoshop_date_created.is_none());
}

/// 引号内 RFC3339 解析失败（非日期字符串）→ None。
#[test]
fn parse_xmp_dates_invalid_rfc3339_returns_none() {
    let xml = r#"<x photoshop:DateCreated="not a date"/>"#;
    assert!(parse_xmp_dates(xml).photoshop_date_created.is_none());
}

/// 起始引号有、终止引号缺 → find 返 None → None。
#[test]
fn parse_xmp_dates_unterminated_quote_returns_none() {
    let xml = r#"<x photoshop:DateCreated="2024-05-01T14:30:00"#;
    assert!(parse_xmp_dates(xml).photoshop_date_created.is_none());
}

/// key 在串尾出现（key+`=` 后已无内容）→ `after_eq.chars().next()` None → None。
#[test]
fn parse_xmp_dates_key_at_end_returns_none() {
    let xml = "aaaphotoshop:DateCreated=";
    assert!(parse_xmp_dates(xml).photoshop_date_created.is_none());
}

// ── strip_xml_comments ──

/// 注释体替换为同字节数空格，前后正文与偏移原样保留。注释起点远离 0
/// 能区分 `i+4` 被变异成 `i*4`（越界→整段误抹）、`p+3` 变异成 `p-3`
/// （尾部 `-->` 残留）等算术错误——仅断言解析结果杀不掉这些变异。
#[test]
fn strip_xml_comments_blanks_mid_text_comment_preserving_offsets() {
    assert_eq!(
        strip_xml_comments("12345<!--x-->after"),
        "12345        after"
    );
}

/// 未闭合注释：剩余内容都按注释体处理 → 整体抹空。
#[test]
fn strip_xml_comments_unterminated_blanks_to_end() {
    let out = strip_xml_comments("abc<!--tail");
    assert!(out.starts_with("abc"));
    assert!(out[3..].chars().all(|c| c == ' '));
}

/// 注释体含多字节 UTF-8 字符（中文）不破坏字符边界。
#[test]
fn strip_xml_comments_multibyte_in_body() {
    let out = strip_xml_comments("p<!-- 中文 -->q");
    assert!(out.starts_with('p'));
    assert!(out.ends_with('q'));
    assert_eq!(out.len(), "p<!-- 中文 -->q".len());
}

/// 无注释直通：每字节按 UTF-8 边界推进，输出与输入逐字节相同。
#[test]
fn strip_xml_comments_no_comments_pass_through() {
    let s = "hello 世界";
    assert_eq!(strip_xml_comments(s), s);
}
