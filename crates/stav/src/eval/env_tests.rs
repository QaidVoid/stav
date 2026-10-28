use super::{Env, Slot, bind, lookup};
use crate::check::model::LocalId;
use crate::eval::val::Val;

fn value(env: &Env, id: u32) -> Option<i64> {
    match lookup(env, LocalId(id)) {
        Some(Slot::Ready(Val::Int(i))) => Some(*i),
        _ => None,
    }
}

#[test]
fn every_bound_local_is_found_whatever_its_number() {
    let ids: Vec<u32> = (0..300).chain([1999, 65_536, u32::MAX]).collect();
    let mut env = Env::default();
    for &id in &ids {
        env = bind(&env, LocalId(id), Slot::Ready(Val::Int(i64::from(id))));
    }
    for &id in &ids {
        assert_eq!(value(&env, id), Some(i64::from(id)));
    }
    assert_eq!(value(&env, 300), None);
    assert_eq!(value(&env, 70_000), None);
}

#[test]
fn binding_again_shadows_and_leaves_the_old_environment_unchanged() {
    let outer = bind(&Env::default(), LocalId(3), Slot::Ready(Val::Int(1)));
    let inner = bind(&outer, LocalId(3), Slot::Ready(Val::Int(2)));
    let wider = bind(&outer, LocalId(500), Slot::Ready(Val::Int(5)));
    assert_eq!(value(&inner, 3), Some(2));
    assert_eq!(value(&outer, 3), Some(1));
    assert_eq!(value(&outer, 500), None);
    assert_eq!((value(&wider, 3), value(&wider, 500)), (Some(1), Some(5)));
}
