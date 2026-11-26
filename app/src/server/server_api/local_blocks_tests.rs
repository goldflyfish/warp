use super::escape;

#[test]
fn exported_terminal_text_cannot_become_html_or_script() {
    assert_eq!(
        escape("<script>\"x\" & 'y'</script>"),
        "&lt;script&gt;&quot;x&quot; &amp; &#39;y&#39;&lt;/script&gt;"
    );
}
