use super::*;
use crate::grammar::names::effects::{EventKind, Phase, Point, Step, Target};
use crate::grammar::names::question::{Budget, NodeRef};

fn entry(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Enter }
}

fn field(node: u32) -> Step {
    Step { node: NodeRef::here(node), point: Point::Field(vec![7]) }
}

fn write(node: u32, binding: u32) -> Step {
    Step {
        node: NodeRef::here(node),
        point: Point::Event(Event {
            kind: EventKind::Write,
            site: NodeRef::here(node),
            target: Some(Target { value: NodeRef::here(binding), binding: Some(NodeRef::here(binding)) }),
            value: None,
            phase: Phase::Leave,
        }),
    }
}

fn graph(steps: Vec<Step>, edges: &[&[StepId]]) -> Execution {
    Execution::new(0, steps, edges.iter().map(|edges| edges.to_vec()).collect())
}

fn writes_binding(binding: u32) -> impl Fn(&Event) -> bool {
    move |event| event.kind == EventKind::Write
        && event.target.is_some_and(|target| target.binding == Some(NodeRef::here(binding)))
}

#[test]
fn branch_proof_does_not_leak_through_a_join_with_an_unguarded_path() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), entry(2)],
        &[&[1, 3], &[2], &[3], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
}

#[test]
fn every_normal_path_must_be_free_of_writes_to_the_tested_binding() {
    let flow = graph(
        vec![entry(0), field(0), write(1, 10), write(2, 11), entry(3)],
        &[&[1], &[2, 3], &[4], &[4], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(10), &question), Known::Yes(false));
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(12), &question), Known::Yes(true));
}

#[test]
fn repeated_guard_tests_refresh_proofs_without_hiding_writes_in_the_current_iteration() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), write(2, 10), entry(3), entry(4)],
        &[&[1], &[2, 5], &[3], &[4], &[1], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 4, writes_binding(10), &question), Known::Yes(false));
    assert_eq!(guarded(&flow, &[1], 5, writes_binding(10), &question), Known::Yes(true));
}

#[test]
fn duplicated_branch_entries_can_jointly_prove_the_same_guard() {
    let flow = graph(
        vec![entry(0), field(0), field(0), entry(1)],
        &[&[1, 2], &[3], &[3], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1, 2], 3, writes_binding(10), &question), Known::Yes(true));
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
}

#[test]
fn unreachable_guard_entries_cannot_invalidate_a_reachable_proof() {
    let flow = graph(
        vec![entry(0), field(0), field(0), write(1, 10), entry(2)],
        &[&[1], &[4], &[3], &[4], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1, 2], 4, writes_binding(10), &question), Known::Yes(true));
}

#[test]
fn missing_paths_and_exhaustion_do_not_create_a_guard_proof() {
    let flow = graph(
        vec![entry(0), field(0), entry(1), entry(2)],
        &[&[1], &[2], &[], &[]],
    );
    let question = QueryContext::default();
    assert_eq!(guarded(&flow, &[1], 3, writes_binding(10), &question), Known::Yes(false));
    let mut exhausted = QueryContext::default();
    exhausted.budget = Budget::new(0);
    assert_eq!(guarded(&flow, &[1], 2, writes_binding(10), &exhausted), Known::Unknown);
    assert_eq!(guarded(&flow, &[1], 99, writes_binding(10), &question), Known::Unknown);
}

#[test]
fn writes_invalidate_only_the_same_runtime_root_and_descendant_member_paths() {
    use crate::grammar::names::question::{FrameId, Target as ValueTarget};
    let root = Leaf { target: ValueTarget::Node(NodeRef::here(10)), frame: FrameId::ROOT };
    let base = Place { root, path: vec![] };
    let member = Place { root, path: vec![Key::Text("payload".into())] };
    let child = Place { root, path: vec![Key::Text("payload".into()), Key::Text("kind".into())] };
    let sibling = Place { root, path: vec![Key::Text("other".into())] };
    let separate = Place {
        root: Leaf { target: ValueTarget::Node(NodeRef::here(11)), ..root },
        ..child.clone()
    };
    assert!(base.invalidates(&child));
    assert!(member.invalidates(&child));
    assert!(child.invalidates(&child));
    assert!(!child.invalidates(&member));
    assert!(!sibling.invalidates(&child));
    assert!(!separate.invalidates(&child));
}
