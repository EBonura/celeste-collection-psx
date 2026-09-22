unsafe fn grapple_check(i: usize, x: Fix32, y: Fix32) -> i32 {
    let tx = (x / fi(8)).floor_int();
    let ty = (y / fi(8)).floor_int();
    let tile = tile_at(tx, ty);
    if backend::fget(tile, 1) {
        OBJ[i].grapple_hit = NONE;
        return if backend::fget(tile, 2) { 2 } else { 1 };
    }
    for j in 0..MAX_OBJ {
        if OBJ[j].exists && !OBJ[j].destroyed && OBJ[j].grapple_mode != 0 && contains(j, x, y) {
            OBJ[i].grapple_hit = j;
            return 1;
        }
    }
    0
}

