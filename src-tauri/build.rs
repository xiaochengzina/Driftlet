fn main() {
    // app 清单改由下面的 compile_for_everything 统一嵌入，这里必须让 tauri 的
    // winres 资源不再带 RT_MANIFEST——否则 bins 会吃到两份 MANIFEST/1
    // （CVTRES CVT1100「资源重复」，LNK1123）。
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("tauri_build failed");

    // comctl32 v6 激活上下文（内容与 tauri 默认 app manifest 相同）。不能只靠
    // tauri_build 内嵌：它只发 rustc-link-arg-bins 覆盖 bins，而 cargo test
    // 的 lib 单元测试二进制没有清单，进程无 v6 激活上下文时 vendored
    // tauri-runtime-wry 消息框静态导入的 TaskDialogIndirect 会解析到
    // System32 的 comctl32 v5（无该导出），加载期即 0xc0000139 起不来。
    // compile_for_everything 发 plain rustc-link-arg，覆盖 lib 测试二进制；
    // rustc-link-arg-tests 只认 tests/ 集成测试目标（本 crate 没有，勿用）。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for_everything("windows/app-manifest.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
