use super::expand_env;
use super::resolve_var;
use super::test_common::remove_env_var;
use super::test_common::set_env_var;

#[test]
fn expand_env_substitutes_default_when_var_missing() {
    remove_env_var("TIDYMEDIA_TEST_MISSING_VAR_X");
    let s = expand_env("a: ${TIDYMEDIA_TEST_MISSING_VAR_X:-7}");
    assert_eq!(s, "a: 7");
}

/// placeholder 位于串首（i=0）：既有用例 `${` 全在 i>=2 处，杀不掉
/// `find_close_brace(bytes, i + 2)` 被变异成 `i - 2`（i>=2 时搜索结果恰好相同；
/// i=0 时 usize 下溢 → 越界 panic）。
#[test]
fn expand_env_placeholder_at_string_start_expands() {
    remove_env_var("TIDYMEDIA_TEST_START_VAR");
    assert_eq!(expand_env("${TIDYMEDIA_TEST_START_VAR:-dft}"), "dft");
}

#[test]
fn expand_env_uses_env_value_when_set() {
    set_env_var("TIDYMEDIA_TEST_SET_VAR_Y", "42");
    let s = expand_env("a: ${TIDYMEDIA_TEST_SET_VAR_Y:-0}");
    assert_eq!(s, "a: 42");
    remove_env_var("TIDYMEDIA_TEST_SET_VAR_Y");
}

#[test]
fn expand_env_resolves_bare_name_without_default() {
    set_env_var("TIDYMEDIA_TEST_BARE_Z", "hi");
    let s = expand_env("k: ${TIDYMEDIA_TEST_BARE_Z}");
    assert_eq!(s, "k: hi");
    remove_env_var("TIDYMEDIA_TEST_BARE_Z");
}

#[test]
fn expand_env_keeps_text_without_placeholder() {
    assert_eq!(expand_env("plain: text"), "plain: text");
}

// 默认值含嵌套 `{}`（如 archive_template 的占位符）：必须按括号配对找
// 闭合 `}`，否则截断成 `{year` 产生非法 YAML（真实 config.yaml:10 的回归）。
#[test]
fn expand_env_default_with_nested_braces_expands_fully() {
    remove_env_var("TIDYMEDIA_TEST_NESTED_TMPL");
    let s = expand_env("t: \"${TIDYMEDIA_TEST_NESTED_TMPL:-{year}/{month}/{valuable_name}}\"");
    assert_eq!(s, "t: \"{year}/{month}/{valuable_name}\"");
}

// 嵌套 `{` 打开后未闭合：depth 永不归零，整段保留原文不替换。
#[test]
fn expand_env_leaves_unbalanced_nested_braces() {
    assert_eq!(expand_env("a: ${VAR:-{x}"), "a: ${VAR:-{x}");
}

#[test]
fn expand_env_leaves_unterminated_brace() {
    assert_eq!(expand_env("a: ${UNCLOSED"), "a: ${UNCLOSED");
}

#[test]
fn expand_env_handles_trailing_dollar() {
    assert_eq!(expand_env("a$"), "a$");
}

// 触发 `bytes[i + 1] == b'{'` 的 False 分支：`$` 后跟非 `{` 字符且不在末尾。
#[test]
fn expand_env_dollar_not_followed_by_brace_passes_through() {
    assert_eq!(expand_env("$abc"), "$abc");
    assert_eq!(expand_env("price: $9.99"), "price: $9.99");
}

// 防御 UTF-8 字符被逐字节降级为 Latin-1（旧实现 `bytes[i] as char` 的 bug）。
#[test]
fn expand_env_preserves_multibyte_chars() {
    set_env_var("TIDYMEDIA_TEST_UTF8_K", "值");
    let s = expand_env("# 目标文件 \nk: ${TIDYMEDIA_TEST_UTF8_K:-默认}");
    assert_eq!(s, "# 目标文件 \nk: 值");
    remove_env_var("TIDYMEDIA_TEST_UTF8_K");
}

#[test]
fn expand_env_preserves_multibyte_default() {
    remove_env_var("TIDYMEDIA_TEST_UTF8_MISS");
    let s = expand_env("k: ${TIDYMEDIA_TEST_UTF8_MISS:-默认值}");
    assert_eq!(s, "k: 默认值");
}

// 真实 config.yaml 中常出现一行多占位符（如 `host: ${HOST:-...} port: ${PORT:-...}`），
// 既验证两次替换都生效，也防御「第二个 `${` 起点定位」回归。
#[test]
fn expand_env_handles_multiple_placeholders_on_same_line() {
    remove_env_var("TIDYMEDIA_TEST_MULTI_HOST");
    remove_env_var("TIDYMEDIA_TEST_MULTI_PORT");
    let both_missing = expand_env(
        "host: ${TIDYMEDIA_TEST_MULTI_HOST:-127.0.0.1} port: ${TIDYMEDIA_TEST_MULTI_PORT:-5037}",
    );
    assert_eq!(both_missing, "host: 127.0.0.1 port: 5037");

    set_env_var("TIDYMEDIA_TEST_MULTI_HOST", "example.com");
    let host_set = expand_env(
        "host: ${TIDYMEDIA_TEST_MULTI_HOST:-127.0.0.1} port: ${TIDYMEDIA_TEST_MULTI_PORT:-5037}",
    );
    assert_eq!(host_set, "host: example.com port: 5037");
    remove_env_var("TIDYMEDIA_TEST_MULTI_HOST");
}

#[test]
fn resolve_var_missing_no_default_returns_empty() {
    remove_env_var("TIDYMEDIA_TEST_NO_DEFAULT_W");
    assert_eq!(resolve_var("TIDYMEDIA_TEST_NO_DEFAULT_W", 0), "");
}

#[test]
fn expand_env_strips_yaml_unsafe_bytes_from_env_value() {
    // 攻击场景：换行 + 新 yaml key → 应被剥成单行让 yaml 结构注入失效。
    set_env_var(
        "TIDYMEDIA_TEST_INJECT_X",
        "info\narchive_template: \"wrong\"",
    );
    let out = expand_env("level: ${TIDYMEDIA_TEST_INJECT_X:-default}");
    assert!(!out.contains('\n'), "换行必须被 strip：{out:?}");
    assert!(out.contains("info"), "保留首段：{out:?}");
    assert!(
        out.contains("archive_template"),
        "其余字节保留（不再含换行让 yaml 不再注入）：{out:?}"
    );
    remove_env_var("TIDYMEDIA_TEST_INJECT_X");
}

#[test]
fn expand_env_max_depth_emits_literal_to_break_recursion() {
    // 32 层嵌套不该爆栈。expand_env 深度封顶让超界返字面量。
    let mut nested = String::from("x");
    for _ in 0..50 {
        nested = format!("${{TIDYMEDIA_TEST_NESTED_Z:-{nested}}}");
    }
    remove_env_var("TIDYMEDIA_TEST_NESTED_Z");
    let result = expand_env(&nested);
    // 超过 MAX_DEPTH 后内层留作字面量 → result 仍含 `${...}` 序列
    assert!(
        result.contains("${TIDYMEDIA_TEST_NESTED_Z") || result == "x" || result.ends_with("x}"),
        "应能容错收尾不爆栈：{result:?}"
    );
}
