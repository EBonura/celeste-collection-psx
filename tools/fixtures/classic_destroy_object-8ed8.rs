// Frozen from8ed8a579c9b21ca22eee45d205c4147b095c1f64 games/celeste/src/game.rs.
unsafe fn destroy_object(o: *mut Obj) {
    // shift later slots left (loading-jank emulation)
    let base = objp(0);
    let mut p = o;
    while p.add(1) < base.add(MAX_OBJECTS) {
        *p = *p.add(1);
        p = p.add(1);
    }
    (*base.add(MAX_OBJECTS - 1)).active = false;
}

