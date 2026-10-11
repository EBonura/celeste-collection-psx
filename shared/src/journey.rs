//! Read-only campaign observations for artifact-bound input journeys.
//!
//! These fields report guest state. They never control game simulation.

#[repr(C)]
pub struct JourneyState {
    pub version: i32,
    pub cart: i32,
    pub updates: i32,
    pub input_latches: i32,
    pub input_mask: i32,
    pub segment: i32,
    pub player_present: i32,
    pub player_x: i32,
    pub player_y: i32,
    pub velocity_x: i32,
    pub velocity_y: i32,
    pub player_state: i32,
    pub deaths: i32,
    pub berries: i32,
    pub berry_bits: i32,
    pub checkpoint: i32,
    pub ability: i32,
    pub ending: i32,
    pub final_score: i32,
    pub debug_fly: i32,
    pub published: i32,
    pub sequence: i32,
}

impl JourneyState {
    pub const fn new(cart: i32) -> Self {
        Self {
            version: 1,
            cart,
            updates: 0,
            input_latches: 0,
            input_mask: 0,
            segment: -1,
            player_present: 0,
            player_x: 0,
            player_y: 0,
            velocity_x: 0,
            velocity_y: 0,
            player_state: -1,
            deaths: 0,
            berries: 0,
            berry_bits: 0,
            checkpoint: -1,
            ability: 0,
            ending: 0,
            final_score: -1,
            debug_fly: 0,
            published: 0,
            sequence: 0,
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<JourneyState>() == 88);
    assert!(core::mem::align_of::<JourneyState>() == 4);
    assert!(core::mem::offset_of!(JourneyState, version) == 0);
    assert!(core::mem::offset_of!(JourneyState, cart) == 4);
    assert!(core::mem::offset_of!(JourneyState, updates) == 8);
    assert!(core::mem::offset_of!(JourneyState, input_latches) == 12);
    assert!(core::mem::offset_of!(JourneyState, input_mask) == 16);
    assert!(core::mem::offset_of!(JourneyState, segment) == 20);
    assert!(core::mem::offset_of!(JourneyState, player_present) == 24);
    assert!(core::mem::offset_of!(JourneyState, player_x) == 28);
    assert!(core::mem::offset_of!(JourneyState, player_y) == 32);
    assert!(core::mem::offset_of!(JourneyState, velocity_x) == 36);
    assert!(core::mem::offset_of!(JourneyState, velocity_y) == 40);
    assert!(core::mem::offset_of!(JourneyState, player_state) == 44);
    assert!(core::mem::offset_of!(JourneyState, deaths) == 48);
    assert!(core::mem::offset_of!(JourneyState, berries) == 52);
    assert!(core::mem::offset_of!(JourneyState, berry_bits) == 56);
    assert!(core::mem::offset_of!(JourneyState, checkpoint) == 60);
    assert!(core::mem::offset_of!(JourneyState, ability) == 64);
    assert!(core::mem::offset_of!(JourneyState, ending) == 68);
    assert!(core::mem::offset_of!(JourneyState, final_score) == 72);
    assert!(core::mem::offset_of!(JourneyState, debug_fly) == 76);
    assert!(core::mem::offset_of!(JourneyState, published) == 80);
    assert!(core::mem::offset_of!(JourneyState, sequence) == 84);
};
