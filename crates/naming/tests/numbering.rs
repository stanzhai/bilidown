//! test_task_naming_snapshot.py numbering block + manager numbering flow —
//! `__get_number` / `__rollback_number` semantics (three modes, global
//! counter always advances, rollback only when nobody took a number since);
//! ported from the Python manager vectors.

use naming::format::Value;
use naming::numbering::{NumberingState, NumberingToken, CONTINUOUS, FROM_SPECIFIED, USE_PARSE_LIST};

#[test]
fn continuous_uses_global_number() {
    let mut state = NumberingState { global: 7, current: None };

    let (n, _) = state.next_number(CONTINUOUS, None);
    assert_eq!(n, Value::Int(7));

    // 全局起始编号自增：每个条目都占一个号，无论后面建任务成不成功
    let (n2, _) = state.next_number(CONTINUOUS, None);
    assert_eq!(n2, Value::Int(8));
}

#[test]
fn from_specified_uses_then_increments_current() {
    let mut state = NumberingState { global: 1, current: Some(10) };

    let (n, _) = state.next_number(FROM_SPECIFIED, None);
    assert_eq!(n, Value::Int(10));

    let (n2, _) = state.next_number(FROM_SPECIFIED, None);
    assert_eq!(n2, Value::Int(11));
}

#[test]
fn use_parse_list_reads_episode_number() {
    let mut state = NumberingState::default();

    let (n, token) = state.next_number(USE_PARSE_LIST, Some(&Value::Text("3-1".into())));
    assert_eq!(n, Value::Text("3-1".into()));
    assert_eq!(token, NumberingToken::None);

    // 没有 number 时取空串，绝不落到全局计数上
    let (n2, _) = state.next_number(USE_PARSE_LIST, None);
    assert_eq!(n2, Value::Text(String::new()));
}

#[test]
fn rollback_restores_when_state_untouched() {
    let mut state = NumberingState { global: 5, current: Some(5) };

    let before = (5, Some(5));
    let (n, token) = state.next_number(FROM_SPECIFIED, None);
    assert_eq!(n, Value::Int(5));
    let _ = before;

    // 建任务失败：号要还回去
    state.rollback(&token);
    assert_eq!(state.global, 5);
    assert_eq!(state.current, Some(5));
}

#[test]
fn rollback_noop_when_someone_else_took_a_number() {
    let mut state = NumberingState { global: 5, current: Some(5) };

    let (_, token) = state.next_number(FROM_SPECIFIED, None);

    // 另一个线程取了号 —— 回退会撞号，宁可在序号里留个洞
    let _ = state.next_number(FROM_SPECIFIED, None);

    state.rollback(&token);
    assert_eq!(state.global, 7);
    assert_eq!(state.current, Some(7));
}

#[test]
fn none_token_is_a_noop() {
    let mut state = NumberingState::default();
    state.rollback(&NumberingToken::None);
    assert_eq!(state, NumberingState::default());
}
