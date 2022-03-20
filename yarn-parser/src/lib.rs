#![doc = include_str!("../README.md")]
pub mod pool;

// TODO(david): 
//  * SIMD parsing
//
/// scan through a .yarn file loaded into memory, finding variables and instantiating them 
/// (with initial values) into a hashmap
#[inline(always)]
pub fn find_variables(yarn: &str) {
    let mut state = false;
    let mut start_idx = 0; // tracks start of a found variable
    for (idx, ch) in yarn.chars().enumerate() {
        if state {
            match ch {
                'A'..='Z' | 'a'..='z' | '0'..='9' => {}, // allowed variable name characters
                _ => {

                }, // variable name captured
            };
            state = false;
        } else {
            if ch == '$' {
                start_idx = idx;
                state = false;
            }
        }
    }
}

pub fn generate_graph() {}

