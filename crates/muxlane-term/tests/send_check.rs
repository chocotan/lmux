#[test]
fn pty_session_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<muxlane_term::PtySession>();
}
