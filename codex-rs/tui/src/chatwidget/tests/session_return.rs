use super::*;

#[tokio::test]
async fn session_return_focused_turn_completion_schedules_idle_notification() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.notify(Notification::AgentTurnComplete {
        response: "done".to_string(),
    });

    assert_matches!(chat.pending_notification, None);
    assert_matches!(
        chat.delayed_completion_notification,
        Some(DelayedCompletionNotification {
            notification: Notification::AgentTurnComplete { ref response },
            ..
        }) if response == "done"
    );
}

#[tokio::test]
async fn session_return_key_paste_and_new_turn_cancel_delayed_completion_notification() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.notify(Notification::AgentTurnComplete {
        response: "done".to_string(),
    });

    chat.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_matches!(chat.delayed_completion_notification, None);

    chat.notify(Notification::AgentTurnComplete {
        response: "done".to_string(),
    });
    chat.handle_paste("hello".to_string());
    assert_matches!(chat.delayed_completion_notification, None);

    chat.notify(Notification::AgentTurnComplete {
        response: "done".to_string(),
    });
    chat.handle_codex_event(Event {
        id: "turn-1".into(),
        msg: EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".to_string(),
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: ModeKind::Default,
        }),
    });
    assert_matches!(chat.delayed_completion_notification, None);
}

#[tokio::test]
async fn session_return_unfocused_and_always_completion_notifications_remain_immediate() {
    let (mut unfocused, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    unfocused.terminal_unfocused_since = Some(Instant::now());

    unfocused.notify(Notification::AgentTurnComplete {
        response: "unfocused".to_string(),
    });

    assert_matches!(
        unfocused.pending_notification,
        Some(Notification::AgentTurnComplete { ref response }) if response == "unfocused"
    );
    assert_matches!(unfocused.delayed_completion_notification, None);

    let (mut always, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    always.config.tui_notifications.condition = NotificationCondition::Always;

    always.notify(Notification::AgentTurnComplete {
        response: "always".to_string(),
    });

    assert_matches!(
        always.pending_notification,
        Some(Notification::AgentTurnComplete { ref response }) if response == "always"
    );
    assert_matches!(always.delayed_completion_notification, None);
}

#[tokio::test]
async fn session_return_does_not_emit_away_summary() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let start = Instant::now();

    chat.add_work_progress("command started".to_string(), "cargo test".to_string());
    chat.handle_terminal_focus_changed_at(/*focused*/ false, start);
    chat.add_work_progress("command completed".to_string(), "cargo test".to_string());
    chat.handle_terminal_focus_changed_at(
        /*focused*/ true,
        start + Duration::from_secs(5 * 60 + 1),
    );

    assert!(
        drain_insert_history(&mut rx).is_empty(),
        "returning from a short away period should not emit a summary"
    );
}
