use gpui::{AppContext, TestAppContext};

/// Prova que o `#[gpui::test]` e o `TestAppContext` funcionam com o GPUI vindo por git, fora do
/// workspace do Zed. Se reprovar por compilação, os testes de UI viram verificação manual.
#[gpui::test]
fn o_test_support_do_gpui_funciona_fora_do_zed(cx: &mut TestAppContext) {
    let contador = cx.new(|_| 0_u32);
    contador.update(cx, |n, _| *n += 1);
    assert_eq!(contador.read_with(cx, |n, _| *n), 1);
}
